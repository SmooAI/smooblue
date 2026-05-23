//! Loopback redirect server.
//!
//! For desktop OAuth we bind `127.0.0.1:<random port>`, register that as
//! the redirect URI in client metadata, and wait for the user-agent to
//! deliver the authorization code via query params.

use crate::error::OAuthError;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

const SUCCESS_HTML: &str = include_str!("../../../assets/callback-success.html");
const ERROR_HTML: &str = include_str!("../../../assets/callback-error.html");

/// Result of awaiting a single callback hit on the loopback server.
#[derive(Debug, Clone)]
pub struct CallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub iss: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

/// Bind on an ephemeral port and return the listener + the chosen URL.
pub async fn bind() -> Result<(TcpListener, String), OAuthError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| OAuthError::LoopbackBind(e.to_string()))?;
    let addr: SocketAddr = listener
        .local_addr()
        .map_err(|e| OAuthError::LoopbackBind(e.to_string()))?;
    let url = format!("http://127.0.0.1:{}/callback", addr.port());
    Ok((listener, url))
}

/// Accept one HTTP request and parse OAuth callback params. Times out
/// after `wait` to keep the UI from hanging forever.
pub async fn await_callback(
    listener: TcpListener,
    wait: Duration,
) -> Result<CallbackParams, OAuthError> {
    let accept = async {
        loop {
            let (mut socket, _) = listener
                .accept()
                .await
                .map_err(|e| OAuthError::CallbackError(format!("accept: {e}")))?;

            let mut buf = vec![0u8; 4096];
            let n = socket
                .read(&mut buf)
                .await
                .map_err(|e| OAuthError::CallbackError(format!("read: {e}")))?;
            let req = String::from_utf8_lossy(&buf[..n]).to_string();

            let request_line = req.lines().next().unwrap_or("");
            let path = request_line.split_whitespace().nth(1).unwrap_or("/");

            if !path.starts_with("/callback") {
                let _ = socket
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }

            let params = parse_query(path);
            let body = if params.error.is_some() {
                ERROR_HTML
            } else {
                SUCCESS_HTML
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(response.as_bytes()).await;
            let _ = socket.shutdown().await;
            return Ok::<_, OAuthError>(params);
        }
    };

    match timeout(wait, accept).await {
        Ok(res) => res,
        Err(_) => Err(OAuthError::CallbackTimeout),
    }
}

fn parse_query(path: &str) -> CallbackParams {
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let map: HashMap<String, String> = query
        .split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((url_decode(k), url_decode(v)))
        })
        .collect();
    CallbackParams {
        code: map.get("code").cloned(),
        state: map.get("state").cloned(),
        iss: map.get("iss").cloned(),
        error: map.get("error").cloned(),
        error_description: map.get("error_description").cloned(),
    }
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                if let Ok(byte) =
                    u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
                {
                    out.push(byte as char);
                    i += 3;
                } else {
                    out.push(bytes[i] as char);
                    i += 1;
                }
            }
            b => {
                out.push(b as char);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn parses_code_and_state_from_callback() {
        let (listener, url) = bind().await.unwrap();
        let port = url
            .split(':')
            .nth(2)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_string();

        // Fire a fake callback request.
        let handle =
            tokio::spawn(async move { await_callback(listener, Duration::from_secs(2)).await });
        let mut sock = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        let req = b"GET /callback?code=abc123&state=xyz&iss=https%3A%2F%2Fbsky.social HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        sock.write_all(req).await.unwrap();
        let mut resp = vec![];
        sock.read_to_end(&mut resp).await.ok();

        let params = handle.await.unwrap().unwrap();
        assert_eq!(params.code.as_deref(), Some("abc123"));
        assert_eq!(params.state.as_deref(), Some("xyz"));
        assert_eq!(params.iss.as_deref(), Some("https://bsky.social"));
        assert!(String::from_utf8_lossy(&resp).contains("200 OK"));
    }

    #[tokio::test]
    async fn surfaces_error_callback() {
        let (listener, url) = bind().await.unwrap();
        let port = url
            .split(':')
            .nth(2)
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let handle =
            tokio::spawn(async move { await_callback(listener, Duration::from_secs(2)).await });
        let mut sock = TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .unwrap();
        sock.write_all(b"GET /callback?error=access_denied&error_description=user%20cancelled HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
            .await
            .unwrap();
        let mut resp = vec![];
        sock.read_to_end(&mut resp).await.ok();
        let params = handle.await.unwrap().unwrap();
        assert_eq!(params.error.as_deref(), Some("access_denied"));
        assert_eq!(params.error_description.as_deref(), Some("user cancelled"));
    }

    #[tokio::test]
    async fn times_out_when_no_callback_arrives() {
        let (listener, _url) = bind().await.unwrap();
        let result = await_callback(listener, Duration::from_millis(50)).await;
        assert!(matches!(result, Err(OAuthError::CallbackTimeout)));
    }
}
