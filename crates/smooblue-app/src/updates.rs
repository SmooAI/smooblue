//! Self-update check — single call to GitHub's releases API on boot,
//! compares the latest tag to `CARGO_PKG_VERSION`, sets a context
//! signal if a newer release exists so the deck can render a small
//! "update available" toast with a link to the release page.
//!
//! No auto-install — the user reads the changelog + downloads the
//! new .app on their own time. Adding auto-update would require a
//! signed updater binary (Sparkle-style) which is a separate pearl.
//!
//! Failure is silent: if the GitHub API is down / rate-limited /
//! the repo's been moved, we just don't show the banner. No reason
//! to surface "update check failed" to the user.

use serde::Deserialize;

/// What the toast needs to render: the new version + a clickable URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateAvailable {
    /// Version tag like "v0.3.0".
    pub tag: String,
    /// GitHub release page URL — opened by the toast on click.
    pub url: String,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// One-shot check against the GitHub releases API. Returns
/// `Some(UpdateAvailable)` if the latest tag is newer than the
/// running binary, `None` otherwise (including all error paths).
///
/// Compares with a simple lexicographic strip of the `v` prefix —
/// our release tags are "vMAJOR.MINOR.PATCH" so `0.2.0 < 0.3.0`
/// lexicographically. Good enough until we get to two-digit patch
/// numbers; if/when we do, swap for a real semver compare.
pub async fn check_for_updates(http: &reqwest::Client) -> Option<UpdateAvailable> {
    let current = env!("CARGO_PKG_VERSION");
    let url = "https://api.github.com/repos/SmooAI/smooblue/releases/latest";
    let resp = http
        .get(url)
        // GitHub requires a User-Agent on every API call.
        .header("User-Agent", "smooblue/0.1 (+https://smoo.ai)")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let release: GitHubRelease = resp.json().await.ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    let latest_clean = release.tag_name.trim_start_matches('v');
    if !is_newer(latest_clean, current) {
        return None;
    }
    Some(UpdateAvailable {
        tag: release.tag_name,
        url: release.html_url,
    })
}

/// `true` when `latest` represents a higher version than `current`.
/// Both inputs are bare-version strings (no leading `v`).
fn is_newer(latest: &str, current: &str) -> bool {
    let l = parse_semver(latest);
    let c = parse_semver(current);
    l > c
}

/// Returns `(major, minor, patch)` — missing components default to 0
/// so "0.2" still compares correctly against "0.2.1". Anything that
/// doesn't parse as a number is treated as 0 to be conservative
/// (don't claim an update when we can't read the tag).
fn parse_semver(s: &str) -> (u32, u32, u32) {
    let mut it = s.split(['.', '-']).take(3);
    let major = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let minor = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let patch = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_simple_minor_bump() {
        assert!(is_newer("0.3.0", "0.2.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn is_newer_same_version() {
        assert!(!is_newer("0.2.0", "0.2.0"));
    }

    #[test]
    fn is_newer_older() {
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn is_newer_patch_bump() {
        assert!(is_newer("0.2.1", "0.2.0"));
    }

    #[test]
    fn parse_semver_handles_partial() {
        assert_eq!(parse_semver("0.2"), (0, 2, 0));
        assert_eq!(parse_semver("1.2.3"), (1, 2, 3));
        assert_eq!(parse_semver("1"), (1, 0, 0));
    }

    #[test]
    fn parse_semver_handles_pre_release_suffix() {
        // "1.0.0-rc.1" — we only care about the numeric prefix.
        assert_eq!(parse_semver("1.0.0-rc.1"), (1, 0, 0));
    }

    #[test]
    fn garbage_input_treated_as_zero() {
        // Conservative: if we can't parse the tag, don't claim it's
        // newer (since 0.0.0 < anything reasonable).
        assert!(!is_newer("garbage", "0.1.0"));
    }
}
