//! Disk + keyring persistence for sessions and column layouts.
//!
//! - Session (which includes the DPoP PKCS8 PEM) → OS keyring under
//!   service `ai.smoo.smooblue`, account `oauth-session`.
//! - Column layout → JSON in the app's config dir.

use smooblue_oauth::Session;

const KEYRING_SERVICE: &str = "ai.smoo.smooblue";
const KEYRING_ACCOUNT: &str = "oauth-session";
const COLUMNS_FILE: &str = "columns.json";

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
