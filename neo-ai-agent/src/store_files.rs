//! Rotated-store file helpers shared by the audit, action and durable-action stores.
//!
//! All three stores persist the same append-only, rotated-generation layout
//! (`<file>.1` newest, `<file>.2`, ...), so the generation arithmetic, the
//! bounded generation scan, the wall-clock helper, the torn-tail repair and the
//! hash rendering live here once instead of being copied into each store module.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Error surface the rotated-store helpers report through.
///
/// The audit, action and durable-action stores each keep their own error enum,
/// so the helpers name only the two failures they can produce rather than
/// forcing one error type on all three.
pub(crate) trait StoreFileError: Sized {
    /// The store's "file or device is unavailable" error.
    fn unavailable() -> Self;

    /// The store's "value could not be serialized" error.
    fn serialization() -> Self;
}

/// Path of a rotated generation, where generation 1 is the most recent archive.
pub(crate) fn generation_path(path: &Path, generation: u32) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{generation}"));
    PathBuf::from(name)
}

/// Discovers retained generations (`.1` newest first) with a defensive scan cap.
pub(crate) fn discover_generations(path: &Path) -> Vec<PathBuf> {
    const MAX_GENERATION_SCAN: u32 = 10_000;
    let mut generations = Vec::new();
    for generation in 1..=MAX_GENERATION_SCAN {
        let candidate = generation_path(path, generation);
        if !candidate.exists() {
            break;
        }
        generations.push(candidate);
    }
    generations
}

/// Milliseconds since the Unix epoch, clamped to at least 1.
///
/// Journals use the value as a record timestamp and as a quarantine expiry base,
/// so a clock before the epoch (or a millisecond count that cannot be
/// represented) must still yield a usable, non-zero instant.
pub fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
        .max(1)
}

/// Opens the platform null device, which a closed handle is swapped in for.
pub(crate) fn null_device_file<E: StoreFileError>() -> Result<File, E> {
    #[cfg(windows)]
    let path = "NUL";
    #[cfg(not(windows))]
    let path = "/dev/null";
    OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| E::unavailable())
}

/// Lowercase hex of the SHA-256 digest of `bytes`.
pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes).as_slice())
}

/// Hex SHA-256 of the JSON encoding of `value`.
pub(crate) fn hash_json_value<E: StoreFileError>(value: &impl Serialize) -> Result<String, E> {
    let encoded = serde_json::to_vec(value).map_err(|_| E::serialization())?;
    Ok(hex_sha256(&encoded))
}

/// Whether `hash` is 32 bytes rendered as hex and not the all-zero digest.
///
/// Every journal, evidence bundle and probe result carries hashes supplied by
/// peers or callers; the all-zero digest is reserved for "unset" and must never
/// be accepted as a real one.
pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash != "00".repeat(32) && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The active-file state a rotated store hands to [`rotate_active_file`].
///
/// The action and durable-action journals embed the same five fields; naming
/// them here lets the rename/retention sequence run once instead of twice.
pub(crate) trait RotatedActiveFile: Sized {
    /// The store's own error type.
    type Error: StoreFileError;

    /// Path of the active journal file.
    fn active_path(&self) -> &Path;

    /// Handle of the active journal file.
    fn active_file(&mut self) -> &mut File;

    /// Number of rotated generations retained on disk.
    fn retained_generations(&self) -> u32;

    /// Installs the fresh active handle and clears the per-file counters.
    fn reset_active_file(&mut self, file: File);
}

/// Rotates the active file to generation `.1`, shifts the retained generations
/// down one slot, drops anything past the retention count, and installs a fresh
/// empty active file.
///
/// The caller appends its own rotation checkpoint afterwards: the checkpoint
/// record is store-specific, the file dance is not.
pub(crate) fn rotate_active_file<S: RotatedActiveFile>(state: &mut S) -> Result<(), S::Error> {
    state
        .active_file()
        .sync_all()
        .map_err(|_| S::Error::unavailable())?;
    // Close the active handle so the rename is portable across platforms.
    let closed = std::mem::replace(state.active_file(), null_device_file::<S::Error>()?);
    drop(closed);
    // Drop generations beyond the retention count (e.g. after the policy shrank).
    let mut stale = state.retained_generations() + 1;
    loop {
        let candidate = generation_path(state.active_path(), stale);
        if !candidate.exists() {
            break;
        }
        std::fs::remove_file(&candidate).map_err(|_| S::Error::unavailable())?;
        stale += 1;
    }
    // Drop the oldest retained generation and shift the remaining ones down.
    let generations = state.retained_generations();
    let oldest = generation_path(state.active_path(), generations);
    if oldest.exists() {
        std::fs::remove_file(&oldest).map_err(|_| S::Error::unavailable())?;
    }
    for generation in (2..=generations).rev() {
        let from = generation_path(state.active_path(), generation - 1);
        if from.exists() {
            std::fs::rename(&from, generation_path(state.active_path(), generation))
                .map_err(|_| S::Error::unavailable())?;
        }
    }
    std::fs::rename(state.active_path(), generation_path(state.active_path(), 1))
        .map_err(|_| S::Error::unavailable())?;
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.active_path())
        .map_err(|_| S::Error::unavailable())?;
    state.reset_active_file(file);
    Ok(())
}

/// Truncates a torn trailing record so the next append starts on a record boundary.
pub(crate) fn repair_unterminated_tail<E: StoreFileError>(path: &Path) -> Result<(), E> {
    if !path.exists() {
        return Ok(());
    }
    let bytes = std::fs::read(path).map_err(|_| E::unavailable())?;
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        return Ok(());
    }
    let valid_length = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |position| position + 1);
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|_| E::unavailable())?;
    file.set_len(valid_length as u64)
        .and_then(|()| file.sync_data())
        .map_err(|_| E::unavailable())
}
