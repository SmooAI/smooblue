//! Disk + keyring persistence for sessions and column layouts.
//!
//! - Session (which includes the DPoP PKCS8 PEM) → OS keyring under
//!   service `ai.smoo.smooblue`, account `oauth-session`.
//! - Column layout → JSON in the app's config dir.
//! - Last handle → plaintext in the app's config dir. Non-secret;
//!   used to pre-fill the login input so users don't retype after
//!   sign-out.

use smooblue_oauth::Session;

const KEYRING_SERVICE: &str = "ai.smoo.smooblue";
const KEYRING_ACCOUNT: &str = "oauth-session";
const COLUMNS_FILE: &str = "columns.json";
const LAST_HANDLE_FILE: &str = "last_handle.txt";
const DRAFT_FILE: &str = "draft.txt";

/// Persist the OAuth session.
pub fn save_session(session: &Session) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
    entry.set_password(&json).map_err(|e| e.to_string())
}

/// Restore the OAuth session if one is stored.
pub fn load_session() -> Option<Session> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

/// Drop the persisted session (sign-out).
pub fn clear_session() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT).map_err(|e| e.to_string())?;
    entry.delete_credential().map_err(|e| e.to_string())
}

pub fn save_columns(cols: &[crate::state::ColumnSpec]) -> Result<(), String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")
        .ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(dir.config_dir()).map_err(|e| e.to_string())?;
    let path = dir.config_dir().join(COLUMNS_FILE);
    let json = serde_json::to_string_pretty(cols).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn load_columns() -> Option<Vec<crate::state::ColumnSpec>> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")?;
    let path = dir.config_dir().join(COLUMNS_FILE);
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

/// Remember the handle the user signed in with. Plain text, non-secret.
/// Used to pre-fill the login input after sign-out so they don't retype.
pub fn save_last_handle(handle: &str) -> Result<(), String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")
        .ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(dir.config_dir()).map_err(|e| e.to_string())?;
    let path = dir.config_dir().join(LAST_HANDLE_FILE);
    std::fs::write(path, handle.trim()).map_err(|e| e.to_string())
}

pub fn load_last_handle() -> Option<String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")?;
    let path = dir.config_dir().join(LAST_HANDLE_FILE);
    let s = std::fs::read_to_string(path).ok()?.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Persist the in-progress compose draft so it survives quitting the
/// app mid-post. Called on every keystroke (file write is cheap; the
/// draft is at most 300 chars). Empty input clears the file rather
/// than writing an empty draft we'd then load back on next boot.
pub fn save_draft(text: &str) -> Result<(), String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")
        .ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(dir.config_dir()).map_err(|e| e.to_string())?;
    let path = dir.config_dir().join(DRAFT_FILE);
    if text.is_empty() {
        // Best-effort cleanup; ignore "not found" because that's the
        // very state we wanted anyway.
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

/// Load any saved draft, or `None` if there isn't one (or the file
/// is just whitespace). Trimming on load so the textarea doesn't
/// inherit a trailing newline the user didn't write.
pub fn load_draft() -> Option<String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")?;
    let path = dir.config_dir().join(DRAFT_FILE);
    let s = std::fs::read_to_string(path).ok()?;
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(test)]
mod tests {
    use crate::state::{ColumnKind, ColumnSpec};

    #[test]
    fn columns_serialize_with_stable_shape() {
        let cols = vec![ColumnSpec::home()];
        let json = serde_json::to_string(&cols).unwrap();
        // The Home variant should appear as `"type":"Home"` so future
        // versions can add fields without breaking on-disk layouts.
        assert!(
            json.contains("\"type\":\"Home\""),
            "kind tag must be stable; got {json}"
        );
        let back: Vec<ColumnSpec> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cols);
    }

    #[test]
    fn column_kinds_can_carry_payload() {
        let c = ColumnSpec {
            id: "q".into(),
            kind: ColumnKind::Search {
                query: "rust".into(),
            },
            title: "rust".into(),
        };
        let json = serde_json::to_string(&c).unwrap();
        let back: ColumnSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }
}
