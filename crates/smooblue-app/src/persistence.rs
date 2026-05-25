//! Session + layout persistence.
//!
//! Sessions live as JSON files in the app's config dir with mode 0600.
//! We previously used the OS keyring (Keychain on macOS) but the ACL
//! is bound to the app's code signature — and our adhoc signature
//! changes on every rebuild, so each install became a "different app"
//! to the Keychain, forcing the user to re-auth after every update.
//! Files survive rebuilds; the threat model (local-only personal app,
//! DPoP key already a sensitive secret stored next to the access
//! token) lines up with how Slack/Discord/etc. cache auth.
//!
//! Layout: `~/Library/Application Support/ai.Smoo.smooblue/`
//!   session.json            ← legacy single-slot session
//!   session-<did>.json      ← per-account session (multi-account)
//!   accounts.json           ← which accounts exist + which is active
//!   columns.json            ← deck layout
//!   last_handle.txt         ← login pre-fill (non-secret)
//!   draft.txt               ← in-progress compose
//!   theme.txt               ← dark / light

use smooblue_oauth::Session;

const ACCOUNTS_FILE: &str = "accounts.json";
const COLUMNS_FILE: &str = "columns.json";
const LAST_HANDLE_FILE: &str = "last_handle.txt";
const DRAFT_FILE: &str = "draft.txt";
const THEME_FILE: &str = "theme.txt";
const SESSION_FILE: &str = "session.json";

fn config_dir() -> Option<std::path::PathBuf> {
    Some(
        directories::ProjectDirs::from("ai", "Smoo", "smooblue")?
            .config_dir()
            .to_path_buf(),
    )
}

/// Atomically write `data` to `path` with mode 0600. Atomic so a
/// crash mid-write doesn't leave a half-truncated session file.
fn write_secret(path: &std::path::Path, data: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, data).map_err(|e| e.to_string())?;
    // Owner-only readable. Best-effort; if the user has a weird umask
    // or non-POSIX FS the file is still written, just less locked-down.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Persist the OAuth session.
pub fn save_session(session: &Session) -> Result<(), String> {
    let path = config_dir().ok_or("no config dir")?.join(SESSION_FILE);
    let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
    write_secret(&path, &json)
}

/// Restore the OAuth session if one is stored.
pub fn load_session() -> Option<Session> {
    let path = config_dir()?.join(SESSION_FILE);
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

/// Drop the persisted session (sign-out).
pub fn clear_session() -> Result<(), String> {
    let path = config_dir().ok_or("no config dir")?.join(SESSION_FILE);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
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

/// Sanitize a DID for use in a filename (slashes / colons aren't
/// portable). bsky DIDs use `:` (e.g. `did:plc:abc`) so we encode
/// them by replacing with `_`.
fn session_filename_for(did: &str) -> String {
    let safe = did.replace([':', '/', '\\'], "_");
    format!("session-{safe}.json")
}

fn accounts_path() -> Option<std::path::PathBuf> {
    Some(config_dir()?.join(ACCOUNTS_FILE))
}

/// Load the multi-account index. On first run after the multi-account
/// migration ships, falls back to migrating a legacy single-account
/// session into a fresh index — the user keeps their signed-in
/// session without re-authing.
pub fn load_accounts() -> Accounts {
    if let Some(path) = accounts_path() {
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(acc) = serde_json::from_str::<Accounts>(&s) {
                return acc;
            }
        }
    }
    // Migrate legacy single-session: if session.json exists, write
    // a keyed copy and synthesize an index pointing at it.
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
    let path = config_dir()
        .ok_or("no config dir")?
        .join(session_filename_for(did));
    let json = serde_json::to_string(session).map_err(|e| e.to_string())?;
    write_secret(&path, &json)
}

pub fn load_session_for(did: &str) -> Option<Session> {
    let path = config_dir()?.join(session_filename_for(did));
    let json = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&json).ok()
}

pub fn delete_session_for(did: &str) -> Result<(), String> {
    let path = config_dir()
        .ok_or("no config dir")?
        .join(session_filename_for(did));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
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
    if s.is_empty() {
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
