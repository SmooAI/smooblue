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
const ACCOUNTS_FILE: &str = "accounts.json";
const COLUMNS_FILE: &str = "columns.json";
const LAST_HANDLE_FILE: &str = "last_handle.txt";
const DRAFT_FILE: &str = "draft.txt";
const THEME_FILE: &str = "theme.txt";

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

/// One known account — kept in a small disk index alongside the
/// keyring entries that store the actual session bytes.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AccountRef {
    pub did: String,
    pub handle: String,
}

/// On-disk index of all known accounts plus which one is currently
/// active. Lives at `accounts.json` in the config dir; the session
/// bytes for each account live in the OS keyring under the account
/// name `oauth-session:<did>`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Accounts {
    pub active_did: Option<String>,
    pub accounts: Vec<AccountRef>,
}

fn keyring_account_for(did: &str) -> String {
    format!("{KEYRING_ACCOUNT}:{did}")
}

fn accounts_path() -> Option<std::path::PathBuf> {
    Some(
        directories::ProjectDirs::from("ai", "Smoo", "smooblue")?
            .config_dir()
            .join(ACCOUNTS_FILE),
    )
}

/// Load the multi-account index. On first run after the multi-account
/// migration ships, falls back to migrating a legacy single-account
/// keyring entry into a fresh index — the user gets to keep their
/// signed-in session without re-authing.
pub fn load_accounts() -> Accounts {
    if let Some(path) = accounts_path() {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(acc) = serde_json::from_str::<Accounts>(&s) {
                return acc;
            }
        }
    }
    // Migrate legacy single-session: if `oauth-session` exists in
    // keyring, write it under the new keyed name and synthesize an
    // index pointing at it.
    if let Some(s) = load_session() {
        let did = s.did.clone();
        let handle = s.handle.clone();
        if save_session_for(&did, &s).is_ok() {
            let accounts = Accounts {
                active_did: Some(did.clone()),
                accounts: vec![AccountRef { did, handle }],
            };
            let _ = save_accounts(&accounts);
            return accounts;
        }
    }
    Accounts::default()
}

pub fn save_accounts(accounts: &Accounts) -> Result<(), String> {
    let path = accounts_path().ok_or_else(|| "no config dir".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(accounts).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Persist a session keyed by DID. Independent of [`save_session`]
/// (legacy single-slot) — multi-account callers should use this.
pub fn save_session_for(did: &str, session: &Session) -> Result<(), String> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, &keyring_account_for(did)).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
    entry.set_password(&json).map_err(|e| e.to_string())
}

pub fn load_session_for(did: &str) -> Option<Session> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, &keyring_account_for(did)).ok()?;
    let json = entry.get_password().ok()?;
    serde_json::from_str(&json).ok()
}

pub fn delete_session_for(did: &str) -> Result<(), String> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, &keyring_account_for(did)).map_err(|e| e.to_string())?;
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

/// Persist the user's chosen theme (currently "dark" or "light").
/// Free-form string on purpose — keeps room for future variants
/// (e.g., "system", "high-contrast") without a migration.
pub fn save_theme(mode: &str) -> Result<(), String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")
        .ok_or_else(|| "no config dir".to_string())?;
    std::fs::create_dir_all(dir.config_dir()).map_err(|e| e.to_string())?;
    let path = dir.config_dir().join(THEME_FILE);
    std::fs::write(path, mode.trim()).map_err(|e| e.to_string())
}

pub fn load_theme() -> Option<String> {
    let dir = directories::ProjectDirs::from("ai", "Smoo", "smooblue")?;
    let path = dir.config_dir().join(THEME_FILE);
    let s = std::fs::read_to_string(path).ok()?.trim().to_string();
    if s.is_empty() { None } else { Some(s) }
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
