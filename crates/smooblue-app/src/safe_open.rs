//! `safe_open` — wrapper around macOS `open` (and Linux `xdg-open`) that
//! refuses anything but http/https.
//!
//! Why: `open` honors every registered URL scheme on the system —
//! `file://`, `mailto:`, `slack://`, `vscode://`, custom protocol
//! handlers — and post external-embed URIs (`ext.uri`) are set by
//! whoever published the post. Without scheme validation, every
//! malicious embed in your feed is one click away from a
//! `file:///Users/<you>/.ssh/id_rsa` Preview pop, a `mailto:` phish
//! reflector, or a deep-link into any installed app.
//!
//! Allowlist (rather than blocklist) by design.

use std::process::Command;

/// Open a URL in the system default browser if and only if its
/// scheme is http or https. Returns `Ok(true)` if launched,
/// `Ok(false)` if blocked by the allowlist, and `Err(reason)` for
/// parser failures.
///
/// We launch fire-and-forget via `spawn` — the user clicked, they
/// don't want to wait for a process exit.
pub fn open_in_browser(url: &str) -> Result<bool, String> {
    if !is_safe_browser_url(url) {
        return Ok(false);
    }
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        // On Windows, `cmd /C start "" <url>` is the standard
        // "open in default browser" invocation. Smooblue doesn't
        // ship Windows builds yet, but it'll just work when it does.
        return Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| true)
            .map_err(|e| e.to_string());
    } else {
        "xdg-open"
    };
    Command::new(cmd)
        .arg(url)
        .spawn()
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// True iff the URL parses cleanly AND its scheme is http or https.
/// Pulled out as a separate fn so we can unit-test it without
/// actually spawning processes.
pub fn is_safe_browser_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    matches!(parsed.scheme(), "http" | "https")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_http_and_https() {
        assert!(is_safe_browser_url("https://bsky.app/profile/alice"));
        assert!(is_safe_browser_url("http://example.com/path?q=1"));
    }

    #[test]
    fn rejects_dangerous_schemes() {
        for url in [
            "file:///Users/victim/.ssh/id_rsa",
            "file:///etc/passwd",
            "mailto:phish@evil.com?subject=hello&body=stealthis",
            "slack://team-1/channel-2",
            "vscode://file/etc/hosts",
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "ssh://attacker@evil.com",
            "x-apple-helpbasic://help",
            "ms-outlook://compose",
            // Custom protocols any installed app might have
            // registered. Allowlist > blocklist.
            "zoommtg://zoom.us/join?action=join&confno=123",
            "tg://join?invite=abc",
        ] {
            assert!(!is_safe_browser_url(url), "should reject {url}");
        }
    }

    #[test]
    fn rejects_malformed_urls() {
        for url in [
            "",
            "not a url",
            "//example.com",
            "://example.com",
            "ht tp://example.com",
        ] {
            assert!(!is_safe_browser_url(url), "should reject {url}");
        }
    }

    #[test]
    fn does_not_strip_username_password_style_urls() {
        // Edge case: http://user:pass@host is a valid scheme but the
        // userinfo can be used to mask the actual host in some UIs.
        // We still allow it (browsers handle the rendering); the
        // scheme allowlist is doing its job by keeping us in http(s).
        assert!(is_safe_browser_url("https://attacker:foo@example.com/"));
    }
}
