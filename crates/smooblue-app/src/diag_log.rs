//! Diagnostic file logging.
//!
//! macOS's unified log drops plain `eprintln!`/stderr writes from
//! GUI apps launched via Finder — only `os_log`/`NSLog` make it
//! through. That's a problem for remote debugging: the user can
//! report "the inbox is empty" but we can't see the ingestion logs
//! unless they relaunch from terminal.
//!
//! This module appends every diagnostic line to a file at
//! `directories::data_dir/smooblue/diag.log`, alongside the existing
//! stderr write. Cap at [`MAX_LOG_BYTES`] — rotates by truncating
//! the front of the file when it grows past the cap. Cheap (open,
//! write, close per line) but bounded, so a year of running can't
//! eat the disk.
//!
//! Usage: `crate::diag_log::log(format_args!("…"))` — same shape as
//! `format_args!` so `format!`-style args work directly.

use parking_lot::Mutex;
use std::fmt::Arguments;
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::OnceLock;

/// Rotate the log when it crosses this size. 1 MB is enough for
/// ~1 week of typical ingestion at the current verbosity (30s
/// cycle × 2-3 lines per cycle × ~80 bytes per line ≈ 60 KB/day).
const MAX_LOG_BYTES: u64 = 1_000_000;

/// Lazily-resolved log file path. None on platforms where ProjectDirs
/// fails (no userland — testing on ephemeral CI containers).
fn log_path() -> Option<PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| {
        let dirs = directories::ProjectDirs::from("ai", "Smoo", "smooblue")?;
        let data_dir = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(&data_dir).ok()?;
        Some(data_dir.join("diag.log"))
    })
    .clone()
}

/// Serialized writer to prevent interleaved lines across threads.
static WRITER: Mutex<()> = Mutex::new(());

/// Append a diagnostic line. Always mirrors to stderr (for users who
/// DO run from terminal) AND to the file. Failures on the file side
/// are silent — diagnostic logging itself should never abort the app.
pub fn log(args: Arguments<'_>) {
    let line = format!("{args}");
    eprintln!("{line}");
    let Some(path) = log_path() else { return };
    let _guard = WRITER.lock();
    let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let _ = writeln!(f, "{ts} {line}");
    // Rotate via front-truncation if we've crossed the cap. Cheap +
    // bounded: read tail half into memory, rewrite the file with it.
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            rotate_in_place(&path);
        }
    }
}

fn rotate_in_place(path: &PathBuf) {
    // Read the LAST MAX_LOG_BYTES/2 bytes, rewrite as the new file.
    // Truncation happens at line boundary best-effort (skip to next
    // newline) so the log stays parseable.
    let Ok(meta) = std::fs::metadata(path) else {
        return;
    };
    let len = meta.len();
    let keep = MAX_LOG_BYTES / 2;
    if len <= keep {
        return;
    }
    let Ok(mut f) = OpenOptions::new().read(true).open(path) else {
        return;
    };
    if f.seek(SeekFrom::End(-(keep as i64))).is_err() {
        return;
    }
    use std::io::Read;
    let mut buf = Vec::with_capacity(keep as usize);
    if f.read_to_end(&mut buf).is_err() {
        return;
    }
    // Skip past partial first line so the file always starts at a
    // line boundary.
    let start = buf
        .iter()
        .position(|&b| b == b'\n')
        .map(|i| i + 1)
        .unwrap_or(0);
    let tail = &buf[start..];
    let _ = std::fs::write(path, tail);
}

/// Macro mirroring `eprintln!` shape. Use this instead of raw
/// `eprintln!` for diagnostic lines we want to grep remotely.
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        $crate::diag_log::log(format_args!($($arg)*))
    };
}
