//! TID (timestamp identifier) decoding.
//!
//! An atproto `rkey` for timestamped collections (posts, follows, likes,
//! reposts …) is a **TID**: a 64-bit value, big-endian base32-`sortable`
//! encoded into 13 characters. The layout is:
//!
//! ```text
//!   bit 63        : always 0 (keeps the value positive / sortable)
//!   bits 62..=10  : microseconds since the UNIX epoch (53 bits)
//!   bits  9..=0   : a per-process "clock identifier" (10 bits)
//! ```
//!
//! Decoding the creation instant of a record from its rkey lets us
//! reconstruct growth curves (follows, posts) without storing a separate
//! `createdAt` for every record. Pure, no IO.
//!
//! Canonical home for this util — there are intentionally **no** inline
//! copies elsewhere (analytics store / scoring / ingest all import this).

use chrono::{DateTime, TimeZone, Utc};

/// The base32-`sortable` alphabet used by atproto TIDs. Index in this
/// string == the 5-bit value of the character. Note this is **not** RFC
/// 4648 base32 — the ordering is chosen so lexical sort == numeric sort.
const TID_ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";

/// A well-formed TID is exactly 13 base32-sortable characters.
const TID_LEN: usize = 13;

/// Map a single base32-sortable character to its 5-bit value, or `None`
/// if the byte isn't in the alphabet.
fn char_value(b: u8) -> Option<u64> {
    TID_ALPHABET.iter().position(|&c| c == b).map(|i| i as u64)
}

/// Decode a TID into **microseconds since the UNIX epoch**.
///
/// Returns `None` if `tid` isn't a syntactically valid TID (wrong length
/// or a character outside the base32-sortable alphabet). The low 10
/// "clock identifier" bits are discarded.
///
/// ```
/// use smooblue_atproto::tid::tid_to_micros;
/// assert!(tid_to_micros("3mp5c2tkblw2p").is_some());
/// assert_eq!(tid_to_micros(""), None);
/// assert_eq!(tid_to_micros("invalid!!!"), None);
/// ```
pub fn tid_to_micros(tid: &str) -> Option<u64> {
    if tid.len() != TID_LEN {
        return None;
    }
    let mut value: u64 = 0;
    for &b in tid.as_bytes() {
        let v = char_value(b)?;
        value = (value << 5) | v;
    }
    // Drop the 10-bit clock identifier; the remaining high bits are the
    // microsecond timestamp (the top bit is always 0 for a valid TID).
    Some(value >> 10)
}

/// Decode a TID into a UTC instant.
///
/// Returns `None` for a malformed TID or a timestamp that overflows
/// `DateTime<Utc>`.
///
/// ```
/// use smooblue_atproto::tid::tid_to_datetime;
/// let dt = tid_to_datetime("3mp5c2tkblw2p").unwrap();
/// assert_eq!(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(), "2026-06-25T20:40:44Z");
/// ```
pub fn tid_to_datetime(tid: &str) -> Option<DateTime<Utc>> {
    let micros = tid_to_micros(tid)?;
    // micros fits in i64 for any realistic TID (53-bit timestamp).
    let micros = i64::try_from(micros).ok()?;
    Utc.timestamp_micros(micros).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_rkey_to_known_date() {
        let dt = tid_to_datetime("3mp5c2tkblw2p").expect("valid TID decodes");
        // Contract's verified case — truncated to whole seconds.
        assert_eq!(
            dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            "2026-06-25T20:40:44Z"
        );
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(tid_to_micros(""), None);
        assert_eq!(tid_to_datetime(""), None);
        // Contains '!' (outside the alphabet) and is the wrong length.
        assert_eq!(tid_to_micros("invalid!!!"), None);
        assert_eq!(tid_to_datetime("invalid!!!"), None);
        // Right length, but '1', '0', '8', '9' aren't in the alphabet.
        assert_eq!(tid_to_micros("3mp5c2tkblw20"), None);
    }

    #[test]
    fn is_monotonic_later_rkey_decodes_later() {
        let earlier = tid_to_datetime("3mp5c2tkblw2p").unwrap();
        // Bump a timestamp character ('w' -> 'x' at index 10, above the
        // low 10 clock-id bits) so the decoded microsecond instant is
        // strictly larger. (Bumping the final two chars would only move
        // the clock-id, which `>> 10` discards.)
        let later = tid_to_datetime("3mp5c2tkblx2p").unwrap();
        assert!(later > earlier, "{later} should be after {earlier}");
    }
}
