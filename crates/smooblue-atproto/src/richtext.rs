//! Rich-text facet detection for `app.bsky.feed.post`.
//!
//! The bsky lexicon stores mentions / links / hashtags as **facets**:
//! byte-range annotations on the post text that other clients use
//! to make those substrings clickable + to fire mention
//! notifications.
//!
//! Without facets, smooblue's posts look broken on bsky.app — @handles
//! aren't clickable, mentioned users aren't notified, hashtags don't
//! land on the hashtag pages, and URLs render as plain text.
//!
//! This module does the **sync** detection: scan the text for
//! candidate ranges + kinds. Mention candidates carry the handle as a
//! string; resolving handle → DID is async + lives in `AtClient`.
//!
//! ## Byte offsets, not char offsets
//!
//! `facet.index.byteStart` / `byteEnd` are **UTF-8 byte** offsets
//! into the post text, not chars or graphemes. This is a routine
//! footgun: a single 😀 is 4 bytes, so a naive `chars().count()`
//! pre-emoji yields the wrong number. Everything here works in
//! `&str` byte indices to avoid the mismatch.

use serde::{Deserialize, Serialize};

/// One detected facet candidate. The `kind` carries the data the
/// caller needs (handle to resolve, link URI, tag name) — the
/// byte range comes straight from the input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetCandidate {
    pub byte_start: usize,
    pub byte_end: usize,
    pub kind: FacetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FacetKind {
    /// `@handle.bsky.social` — caller resolves to a DID via
    /// `com.atproto.identity.resolveHandle` before posting.
    Mention { handle: String },
    /// `https://example.com/foo` — already a full URL, no resolution
    /// needed.
    Link { uri: String },
    /// `#rust` — stored without the `#` prefix per the lexicon.
    Tag { tag: String },
}

/// A bsky facet, ready to embed in a post record. Serialized
/// straight to the lexicon shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Facet {
    pub index: FacetIndex,
    pub features: Vec<FacetFeature>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetIndex {
    #[serde(rename = "byteStart")]
    pub byte_start: usize,
    #[serde(rename = "byteEnd")]
    pub byte_end: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "$type")]
pub enum FacetFeature {
    #[serde(rename = "app.bsky.richtext.facet#mention")]
    Mention { did: String },
    #[serde(rename = "app.bsky.richtext.facet#link")]
    Link { uri: String },
    #[serde(rename = "app.bsky.richtext.facet#tag")]
    Tag { tag: String },
}

/// Scan `text` for all mention / link / tag candidates. Returned in
/// ascending byte-start order (which is what the lexicon expects).
/// Detection is intentionally a little conservative — URL boundaries
/// in particular vary across clients; we match the smallest run that
/// looks safe and let bsky's link card system handle ambiguity.
pub fn detect_facet_candidates(text: &str) -> Vec<FacetCandidate> {
    let mut out = Vec::new();
    out.extend(detect_mentions(text));
    out.extend(detect_links(text));
    out.extend(detect_tags(text));
    out.sort_by_key(|c| c.byte_start);
    // Drop overlaps — if a URL contains a `#anchor`, we don't also
    // want a Tag facet for it. Keep the earlier (link) candidate.
    let mut deduped: Vec<FacetCandidate> = Vec::with_capacity(out.len());
    for c in out {
        let overlaps_last = deduped
            .last()
            .map(|prev| c.byte_start < prev.byte_end)
            .unwrap_or(false);
        if !overlaps_last {
            deduped.push(c);
        }
    }
    deduped
}

/// `@handle.tld` mentions. Bsky handles are DNS-style (must contain
/// at least one dot — `bob.bsky.social`, `paul.frazee.com`, etc.) so
/// a bare `@bob` doesn't match. This matches bsky.app's own
/// detection rule.
fn detect_mentions(text: &str) -> Vec<FacetCandidate> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'@' {
            i += 1;
            continue;
        }
        // The `@` must be at a token boundary — start of text, or
        // preceded by whitespace / a punctuation char. Otherwise
        // it's an email address ("foo@bar.com") or similar, not a
        // mention.
        let prev_ok = i == 0
            || matches!(
                bytes[i - 1],
                b' ' | b'\n' | b'\t' | b'(' | b'[' | b'{' | b','
            );
        if !prev_ok {
            i += 1;
            continue;
        }
        let handle_start = i + 1;
        let mut j = handle_start;
        while j < bytes.len() && is_handle_byte(bytes[j]) {
            j += 1;
        }
        // Strip trailing dot (e.g. end of sentence).
        let mut end = j;
        while end > handle_start && bytes[end - 1] == b'.' {
            end -= 1;
        }
        if end <= handle_start {
            i = j.max(i + 1);
            continue;
        }
        let handle = &text[handle_start..end];
        // Require at least one dot — DNS-style.
        if handle.contains('.') && handle.len() >= 4 {
            out.push(FacetCandidate {
                byte_start: i,
                byte_end: end,
                kind: FacetKind::Mention {
                    handle: handle.to_string(),
                },
            });
        }
        i = end.max(i + 1);
    }
    out
}

fn is_handle_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_'
}

/// `https://example.com/foo` links. Matches a literal `http://` or
/// `https://` followed by any run of non-whitespace; trims trailing
/// punctuation that's almost certainly sentence boundary, not URL.
fn detect_links(text: &str) -> Vec<FacetCandidate> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        // Look for an "http" prefix; we only support http+https schemes
        // because that's what bsky's link cards understand.
        if i + 7 < bytes.len()
            && (&bytes[i..i + 7] == b"http://"
                || (i + 8 <= bytes.len() && &bytes[i..i + 8] == b"https://"))
        {
            // Require start-of-text or whitespace/punctuation
            // before — otherwise we're matching a URL embedded inside
            // another token, which isn't a real link.
            let prev_ok = i == 0
                || matches!(
                    bytes[i - 1],
                    b' ' | b'\n' | b'\t' | b'(' | b'[' | b'{' | b'<' | b'"'
                );
            if !prev_ok {
                i += 1;
                continue;
            }
            let mut j = i;
            while j < bytes.len() && !matches!(bytes[j], b' ' | b'\n' | b'\t') {
                j += 1;
            }
            // Trim trailing sentence punctuation. Conservative — we
            // keep slashes / hashes / etc since those are legal URL
            // bytes; only strip what's almost certainly a sentence end.
            let mut end = j;
            while end > i
                && matches!(
                    bytes[end - 1],
                    b'.' | b',' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'}' | b'"' | b'\''
                )
            {
                end -= 1;
            }
            if end > i + 8 {
                let uri = &text[i..end];
                out.push(FacetCandidate {
                    byte_start: i,
                    byte_end: end,
                    kind: FacetKind::Link {
                        uri: uri.to_string(),
                    },
                });
            }
            i = end.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// `#hashtag` tags. Stored without the leading `#`. Matches Unicode
/// letters/digits/underscores so non-English tags work — the bsky
/// lexicon doesn't restrict to ASCII.
fn detect_tags(text: &str) -> Vec<FacetCandidate> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'#' {
            i += 1;
            continue;
        }
        let prev_ok = i == 0
            || matches!(
                bytes[i - 1],
                b' ' | b'\n' | b'\t' | b'(' | b'[' | b'{' | b','
            );
        if !prev_ok {
            i += 1;
            continue;
        }
        // Match the longest run of "tag bytes". We use a char-aware
        // walk so multibyte letters work; convert back to byte index
        // for the facet range.
        let after_hash = i + 1;
        let mut tag_end = after_hash;
        let tail = &text[after_hash..];
        for (off, c) in tail.char_indices() {
            if c.is_alphanumeric() || c == '_' {
                tag_end = after_hash + off + c.len_utf8();
            } else {
                break;
            }
        }
        if tag_end > after_hash {
            let tag = &text[after_hash..tag_end];
            // Skip pure-number runs ("#42" is almost never a tag);
            // matches bsky.app.
            if !tag.chars().all(|c| c.is_ascii_digit()) {
                out.push(FacetCandidate {
                    byte_start: i,
                    byte_end: tag_end,
                    kind: FacetKind::Tag {
                        tag: tag.to_string(),
                    },
                });
            }
        }
        i = tag_end.max(i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only helper, kept for future tests that want to assert
    /// the full (start, end, kind) tuple instead of the per-field
    /// asserts the current tests use.
    #[allow(dead_code)]
    fn ranges(out: &[FacetCandidate]) -> Vec<(usize, usize, &FacetKind)> {
        out.iter()
            .map(|c| (c.byte_start, c.byte_end, &c.kind))
            .collect()
    }

    #[test]
    fn detects_a_single_mention() {
        let out = detect_facet_candidates("hello @alice.bsky.social!");
        assert_eq!(out.len(), 1);
        let FacetKind::Mention { handle } = &out[0].kind else {
            panic!("expected mention")
        };
        assert_eq!(handle, "alice.bsky.social");
        assert_eq!(out[0].byte_start, 6);
        assert_eq!(out[0].byte_end, 24);
    }

    #[test]
    fn skips_email_addresses() {
        // The `@` is preceded by a letter — not a mention.
        let out = detect_facet_candidates("ping foo@bar.com please");
        assert!(out.is_empty(), "should not match email-like text: {out:?}");
    }

    #[test]
    fn requires_dns_dot_in_handle() {
        // Bare `@bob` shouldn't match — bsky handles need a dot.
        let out = detect_facet_candidates("hi @bob");
        assert!(out.is_empty());
    }

    #[test]
    fn detects_link_with_trailing_punctuation_stripped() {
        let text = "see https://example.com/foo. great";
        let out = detect_facet_candidates(text);
        assert_eq!(out.len(), 1);
        let FacetKind::Link { uri } = &out[0].kind else {
            panic!("expected link")
        };
        assert_eq!(uri, "https://example.com/foo");
        // Verify the byte range matches the trimmed URL, not the
        // original-with-period.
        assert_eq!(
            &text[out[0].byte_start..out[0].byte_end],
            "https://example.com/foo"
        );
    }

    #[test]
    fn detects_a_hashtag() {
        let out = detect_facet_candidates("loving #rust today");
        assert_eq!(out.len(), 1);
        let FacetKind::Tag { tag } = &out[0].kind else {
            panic!("expected tag")
        };
        assert_eq!(tag, "rust");
    }

    #[test]
    fn skips_numeric_only_tags() {
        let out = detect_facet_candidates("see #42 below");
        assert!(
            out.is_empty(),
            "purely numeric tags shouldn't match: {out:?}"
        );
    }

    #[test]
    fn handles_emoji_correctly_in_byte_ranges() {
        // 🦀 is 4 bytes (U+1F980, F0 9F A6 80). The mention starts
        // at byte 5, not char 5 / grapheme 2.
        let text = "🦀 @alice.bsky.social";
        let out = detect_facet_candidates(text);
        assert_eq!(out.len(), 1);
        // 🦀 = 4 bytes + 1 space = byte 5.
        assert_eq!(out[0].byte_start, 5);
        assert_eq!(
            &text[out[0].byte_start..out[0].byte_end],
            "@alice.bsky.social"
        );
    }

    #[test]
    fn detects_mention_link_tag_in_one_post() {
        let text = "hey @alice.bsky.social check https://smoo.ai/blog/x — about #rust";
        let out = detect_facet_candidates(text);
        let kinds: Vec<&FacetKind> = out.iter().map(|c| &c.kind).collect();
        assert_eq!(kinds.len(), 3);
        assert!(matches!(kinds[0], FacetKind::Mention { .. }));
        assert!(matches!(kinds[1], FacetKind::Link { .. }));
        assert!(matches!(kinds[2], FacetKind::Tag { .. }));
    }

    #[test]
    fn link_inside_link_card_text_is_one_facet() {
        // No URL inside another URL — the run-trim should not produce
        // duplicate overlapping facets.
        let out = detect_facet_candidates("https://x.com/y/z https://other.com/path");
        assert_eq!(out.len(), 2);
        assert!(out[0].byte_end <= out[1].byte_start);
    }

    #[test]
    fn skips_links_glued_to_preceding_word() {
        // "barhttps://..." — no whitespace before, so we shouldn't
        // mis-detect a link in the middle of a word.
        let out = detect_facet_candidates("foohttps://example.com/x");
        assert!(
            out.is_empty(),
            "should not match link glued to preceding text: {out:?}"
        );
    }

    #[test]
    fn url_anchor_does_not_create_a_tag_facet() {
        // The `#fragment` inside a URL should not be detected as a
        // separate Tag facet — overlap dedupe drops it.
        let out = detect_facet_candidates("https://example.com/page#section here");
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0].kind, FacetKind::Link { .. }));
    }

    #[test]
    fn empty_string_yields_no_facets() {
        assert!(detect_facet_candidates("").is_empty());
    }

    #[test]
    fn plain_text_yields_no_facets() {
        assert!(detect_facet_candidates("just a regular post with no markup").is_empty());
    }
}
