//! End-to-end OAuth flow against a wiremock'd PDS + authorization server.
//!
//! Exercises: identity resolution → PRM → auth-server metadata → PAR (with
//! `use_dpop_nonce` retry) → token exchange with DPoP-bound tokens.
//!
//! The "browser" side of the flow is simulated by a task that, after PAR
//! succeeds, reads back the state + redirect_uri the client posted to PAR
//! and fires a synthetic callback at the loopback redirect URI.

use smooblue_oauth::{OAuthClient, OAuthClientConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use url::Url;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn parse_form(body: &[u8]) -> HashMap<String, String> {
    let s = std::str::from_utf8(body).unwrap_or("");
    s.split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((urldecode(k), urldecode(v)))
        })
        .collect()
}

fn urldecode(s: &str) -> String {
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

#[tokio::test]
async fn full_oauth_flow_with_dpop_nonce_retry_yields_dpop_session() {
    // ---- Fake AppView + PDS / auth server ----
    let appview = MockServer::start().await;
    let pds = MockServer::start().await;

    // 1. resolveHandle → did:web (so the DID doc lives on the PDS host).
    let pds_host_port = Url::parse(&pds.uri()).unwrap();
    let pds_did = format!(
        "did:web:{}:{}",
        pds_host_port.host_str().unwrap().replace('.', "%2E"),
        pds_host_port.port_or_known_default().unwrap()
    );
    // did:web with a port uses %3A encoding in the spec but most resolvers use
    // explicit colon-mapping; here we lean on resolve_pds_from_did's
    // ':'-to-'/' translation:  did:web:host:port → https://host/port/.well-known/did.json
    // which won't match a real PDS. So skip the did:web roundtrip and instead
    // hand the resolver a `did:web:<pds host>` and serve did.json from the PDS root.
    let pds_host = pds_host_port.host_str().unwrap().to_string();
    let pds_port = pds_host_port.port().unwrap();
    let did = format!("did:web:{pds_host}%3A{pds_port}");
    let _ = pds_did;

    Mock::given(method("GET"))
        .and(path("/xrpc/com.atproto.identity.resolveHandle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "did": did })))
        .mount(&appview)
        .await;

    // 2. did.json on the PDS (covers did:web → service endpoint).
    Mock::given(method("GET"))
        .and(path("/.well-known/did.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "service": [{
                "id": "#atproto_pds",
                "type": "AtprotoPersonalDataServer",
                "serviceEndpoint": pds.uri()
            }]
        })))
        .mount(&pds)
        .await;

    // 3. PRM on the PDS — authorization server = PDS.
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": pds.uri(),
            "authorization_servers": [pds.uri()]
        })))
        .mount(&pds)
        .await;

    // 4. Authorization server metadata.
    let par_endpoint = format!("{}/oauth/par", pds.uri());
    let token_endpoint = format!("{}/oauth/token", pds.uri());
    let auth_endpoint = format!("{}/oauth/authorize", pds.uri());
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": pds.uri(),
            "authorization_endpoint": auth_endpoint,
            "token_endpoint": token_endpoint,
            "pushed_authorization_request_endpoint": par_endpoint,
            "require_pushed_authorization_requests": true,
            "dpop_signing_alg_values_supported": ["ES256"],
            "scopes_supported": ["atproto"]
        })))
        .mount(&pds)
        .await;

    // 5. PAR: first call demands `use_dpop_nonce`, second succeeds.
    let par_attempts: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
    let par_attempts_c = par_attempts.clone();
    Mock::given(method("POST"))
        .and(path("/oauth/par"))
        .and(header_exists("DPoP"))
        .respond_with(move |_req: &Request| {
            let mut n = par_attempts_c.lock().unwrap();
            *n += 1;
            if *n == 1 {
                ResponseTemplate::new(400)
                    .insert_header("DPoP-Nonce", "nonce-1")
                    .set_body_json(serde_json::json!({ "error": "use_dpop_nonce", "error_description": "need nonce" }))
            } else {
                ResponseTemplate::new(201).set_body_json(serde_json::json!({
                    "request_uri": "urn:ietf:params:oauth:request_uri:abc123",
                    "expires_in": 60
                }))
            }
        })
        .mount(&pds)
        .await;

    // 6. Token endpoint succeeds on first try.
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(header_exists("DPoP"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("DPoP-Nonce", "nonce-2")
                .set_body_json(serde_json::json!({
                    "access_token": "at-real-token",
                    "refresh_token": "rt-real-token",
                    "token_type": "DPoP",
                    "expires_in": 3600,
                    "scope": "atproto"
                })),
        )
        .mount(&pds)
        .await;

    // ---- Drive sign-in. The browser stub polls PAR mock requests to
    //      learn the state + redirect_uri the client sent, then fires the
    //      callback. ----
    let cfg = OAuthClientConfig {
        client_id: "https://smoo.ai/smooblue/client-metadata.json".into(),
        appview: Url::parse(&appview.uri()).unwrap(),
        scopes: vec!["atproto".into()],
        callback_timeout: Duration::from_secs(10),
    };
    let client = OAuthClient::new(cfg);

    // The "browser" task gets the MockServer handle so it can poll
    // received_requests() to learn the redirect_uri + state the client posted.
    let mock = Arc::new(pds);
    let mock_for_browser = mock.clone();
    let session = client
        .sign_in("alice.bsky.test", move |auth_url| {
            // Sanity-check the URL the client opens.
            assert!(
                auth_url.contains("/oauth/authorize"),
                "should open authorize endpoint, got: {auth_url}"
            );
            assert!(
                auth_url.contains("request_uri="),
                "authorize URL must carry request_uri"
            );
            let mock = mock_for_browser.clone();
            tokio::spawn(async move {
                for _ in 0..100 {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let requests = mock.received_requests().await.unwrap_or_default();
                    // Find the *successful* PAR call — second one.
                    let par_calls: Vec<_> = requests
                        .iter()
                        .filter(|r| {
                            r.url.path() == "/oauth/par"
                                && r.method.as_str().eq_ignore_ascii_case("POST")
                        })
                        .collect();
                    if par_calls.len() < 2 {
                        continue;
                    }
                    let form = parse_form(&par_calls[1].body);
                    let state = form.get("state").cloned().unwrap_or_default();
                    let redirect_uri = form.get("redirect_uri").cloned().unwrap_or_default();
                    if state.is_empty() || redirect_uri.is_empty() {
                        continue;
                    }
                    let callback = format!(
                        "{}?code=auth-code-xyz&state={}&iss=oauth",
                        redirect_uri, state
                    );
                    let _ = reqwest::Client::new().get(&callback).send().await;
                    return;
                }
            });
            Ok(())
        })
        .await
        .expect("sign-in should complete end-to-end");

    let par_calls = *par_attempts.lock().unwrap();
    assert!(
        par_calls >= 2,
        "PAR should have retried after use_dpop_nonce; got {par_calls}"
    );
    assert_eq!(session.access_token, "at-real-token");
    assert_eq!(session.refresh_token, "rt-real-token");
    assert_eq!(session.token_type, "DPoP");
    assert_eq!(session.dpop_nonce.as_deref(), Some("nonce-2"));
    assert!(session.expires_at > chrono::Utc::now().timestamp() + 3500);
    assert!(
        !session.dpop_pem.is_empty(),
        "DPoP PKCS8 PEM must be persisted for refresh"
    );
    assert!(
        session.did.starts_with("did:web:"),
        "session should carry resolved DID"
    );
}

/// Lower-level: the PAR request body contains the exact fields ATproto requires.
#[tokio::test]
async fn par_request_body_carries_required_fields() {
    let captured: Arc<Mutex<Option<HashMap<String, String>>>> = Arc::new(Mutex::new(None));
    let captured_c = captured.clone();
    let mock = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/oauth/par"))
        .respond_with(move |req: &Request| {
            *captured_c.lock().unwrap() = Some(parse_form(&req.body));
            ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "request_uri": "urn:test",
                "expires_in": 60
            }))
        })
        .mount(&mock)
        .await;

    // Mount the upstream metadata that the client walks before PAR.
    Mock::given(method("GET"))
        .and(path("/xrpc/com.atproto.identity.resolveHandle"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "did": format!("did:web:{}%3A{}", Url::parse(&mock.uri()).unwrap().host_str().unwrap(), Url::parse(&mock.uri()).unwrap().port().unwrap())
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/did.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "service": [{ "id": "#atproto_pds", "type": "AtprotoPersonalDataServer", "serviceEndpoint": mock.uri() }]
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-protected-resource"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "resource": mock.uri(), "authorization_servers": [mock.uri()]
        })))
        .mount(&mock)
        .await;
    Mock::given(method("GET"))
        .and(path("/.well-known/oauth-authorization-server"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "issuer": mock.uri(),
            "authorization_endpoint": format!("{}/oauth/authorize", mock.uri()),
            "token_endpoint": format!("{}/oauth/token", mock.uri()),
            "pushed_authorization_request_endpoint": format!("{}/oauth/par", mock.uri()),
            "require_pushed_authorization_requests": true,
            "dpop_signing_alg_values_supported": ["ES256"]
        })))
        .mount(&mock)
        .await;

    let client = OAuthClient::new(OAuthClientConfig {
        client_id: "https://smoo.ai/smooblue/client-metadata.json".into(),
        appview: Url::parse(&mock.uri()).unwrap(),
        scopes: vec!["atproto".into(), "transition:generic".into()],
        callback_timeout: Duration::from_millis(100),
    });

    // We expect the callback to time out — we only care about the PAR body.
    let _ = client.sign_in("alice.bsky.test", |_| Ok(())).await;

    let body = captured.lock().unwrap().clone().expect("PAR was called");
    assert_eq!(body.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(
        body.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert!(
        body.contains_key("code_challenge"),
        "PKCE challenge missing"
    );
    assert!(body.contains_key("state"), "state missing");
    assert!(body.contains_key("redirect_uri"), "redirect_uri missing");
    assert!(
        body.get("redirect_uri")
            .unwrap()
            .starts_with("http://127.0.0.1:"),
        "redirect must be loopback"
    );
    assert!(body.get("scope").unwrap().contains("atproto"));
    assert_eq!(
        body.get("client_id").map(String::as_str),
        Some("https://smoo.ai/smooblue/client-metadata.json")
    );
}
