//! External rollback floor for sealed-blob replay protection (T43).
//!
//! Security debt fixed here: `SealedKey::unseal` used to derive its replay
//! floor from the blob's own `counter` field (`min_counter =
//! Some(sealed_data.counter)`). That check is self-referential — the floor is
//! exactly the value it is compared against — so it can never reject. After
//! the host-side monotonic counter file was rolled back (an older copy of the
//! sealed-data directory restored, or the counter file deleted/truncated), a
//! previously sealed blob still unsealed successfully.
//!
//! This module adds an independent, atomically persisted *rollback floor*
//! witness stored OUTSIDE the sealed blob, in the enclave's sealed-data
//! directory (`.rollback_floor`). It records the highest monotonic counter
//! value covered by a successful seal:
//!
//! * `SealedKey::seal` raises the floor to `max(floor, counter)` with a
//!   durable atomic write before the sealed key is returned; if the floor
//!   cannot be persisted the seal fails closed.
//! * `SealedKey::unseal` accepts a blob only when
//!   `blob.counter <= floor <= enclave.current_counter()`:
//!   - `floor > enclave counter` proves the counter state was rolled back
//!     relative to the floor witness -> reject;
//!   - `blob.counter > floor` proves the blob is not covered by the floor
//!     history (floor file rolled back, or a blob from a foreign directory)
//!     -> reject.
//! * A corrupt or unparsable floor file is NEVER silently treated as 0:
//!   unseal fails closed with a distinct `TeeError::RollbackProtection`.
//! * A missing floor file is only allowed at first-boot initialization,
//!   where `TeeEnclave::initialize` records an initial floor of 0. A counter
//!   history without a floor witness (legacy pre-floor directory, or a
//!   deleted floor) fails initialization, and unsealing a blob whose counter
//!   is not covered by a floor file also fails closed.
//!
//! # Why `blob.counter <= floor` instead of a strict `blob.counter > floor`
//!
//! The floor equals the counter of the most recent seal, so requiring the
//! blob counter to be strictly greater than the floor would make every
//! sealed key single-use: the freshly sealed key itself (counter == floor)
//! could never be unsealed, and older — still legitimate — wallet keys would
//! be permanently bricked. The floor is still strictly external: the blob's
//! own counter value never feeds the floor check, so removing the blob's
//! self-reference closes the T07-T4 hole while keeping repeated unseals of
//! live keys (`TeeWallet::sign`) working.
//!
//! # Threat model and documented limitations
//!
//! * In simulation mode this is *best-effort, file-level* rollback
//!   detection. The floor is a host file in the same directory as the
//!   counter file, so an attacker who can atomically restore a coherent old
//!   snapshot of the WHOLE sealed-data directory (floor + counter file
//!   together) resets both witnesses; no file-only mechanism can detect
//!   that. What this floor does catch are the partial rollbacks the
//!   self-referential check missed: counter-file-only rollback (floor stays
//!   ahead), floor-file-only rollback (blob no longer covered), truncated/
//!   bit-flipped floor files (fail closed), and blobs replayed from a
//!   different directory.
//! * Real rollback resistance requires a hardware-protected monotonic
//!   counter (e.g. Intel SGX trusted monotonic counters via platform
//!   services). The strict hardware path currently derives the sealing key
//!   from verified quote evidence but has NO hardware monotonic-counter
//!   integration; the same file-based floor is applied there as
//!   defense-in-depth. Wiring SGX trusted counters into `TeeEnclave` remains
//!   a documented gap (see the crate-level SGX status notes).

use crate::error::{TeeError, TeeResult};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// File name of the rollback floor witness inside the sealed-data directory.
pub const FLOOR_FILE_NAME: &str = ".rollback_floor";

/// Fixed width of the floor file: 16-byte header + 32-byte SHA-256 digest.
const FLOOR_FILE_LEN: usize = 48;
const HEADER_LEN: usize = 16;
/// Magic bytes: "Neo Tee Rollback Floor".
const MAGIC: &[u8; 4] = b"NTRF";
const VERSION: u8 = 1;

/// Path of the rollback floor witness for a sealed-data directory.
pub fn floor_path(sealed_data_path: &Path) -> PathBuf {
    sealed_data_path.join(FLOOR_FILE_NAME)
}

/// Encode a floor value: `[magic(4)][version(1)][reserved(3)][counter LE(8)]
/// [SHA-256(header)(32)]`.
///
/// The digest detects truncation and bit rot (an attacker with write access
/// to the directory can of course rewrite the whole file — this is a
/// consistency witness, not a tamper-proof seal; see the module docs).
fn encode(value: u64) -> [u8; FLOOR_FILE_LEN] {
    let mut buf = [0u8; FLOOR_FILE_LEN];
    buf[..4].copy_from_slice(MAGIC);
    buf[4] = VERSION;
    // buf[5..8] stay zero (reserved, must verify zero on load)
    buf[8..HEADER_LEN].copy_from_slice(&value.to_le_bytes());
    let digest = Sha256::digest(&buf[..HEADER_LEN]);
    buf[HEADER_LEN..].copy_from_slice(&digest);
    buf
}

/// Strictly parse a floor file. Any deviation (length, magic, version,
/// reserved bytes, digest) is a corrupt witness -> `TeeError::RollbackProtection`.
fn decode(bytes: &[u8]) -> TeeResult<u64> {
    let corrupt = |detail: &str| {
        TeeError::RollbackProtection(format!(
            "rollback floor witness file is corrupt ({detail}); refusing to proceed (fail-closed)"
        ))
    };

    if bytes.len() != FLOOR_FILE_LEN {
        return Err(corrupt(&format!(
            "expected {} bytes, found {}",
            FLOOR_FILE_LEN,
            bytes.len()
        )));
    }
    if &bytes[..4] != MAGIC {
        return Err(corrupt("bad magic"));
    }
    if bytes[4] != VERSION {
        return Err(corrupt(&format!("unsupported version {}", bytes[4])));
    }
    if bytes[5..8].iter().any(|b| *b != 0) {
        return Err(corrupt("reserved bytes are non-zero"));
    }
    let digest = Sha256::digest(&bytes[..HEADER_LEN]);
    if &bytes[HEADER_LEN..] != digest.as_slice() {
        return Err(corrupt("digest mismatch"));
    }

    Ok(u64::from_le_bytes(bytes[8..HEADER_LEN].try_into().expect(
        "slice length is exactly 8 between offsets 8 and 16",
    )))
}

/// Load the rollback floor witness.
///
/// * `Ok(None)` — the floor file does not exist (first boot).
/// * `Ok(Some(value))` — witness parsed and verified.
/// * `Err` — the file exists but is corrupt/unreadable. Callers MUST fail
///   closed; treating this as 0 would silently disable rollback detection.
pub fn load(path: &Path) -> TeeResult<Option<u64>> {
    match fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(TeeError::IoError(e)),
        Ok(bytes) => decode(&bytes).map(Some),
    }
}

/// Atomically persist the rollback floor witness.
///
/// Write-through order (mirrors the `put_sync`/durable-rename pattern used by
/// `neo-consensus` and `neo-core`'s durable stores):
///
/// 1. write `path + ".tmp"` in the same directory,
/// 2. `sync_all` so the content reaches storage before the swap,
/// 3. durable replace of the target (POSIX `rename` + parent-dir `sync_all`;
///    Windows `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING |
///    MOVEFILE_WRITE_THROUGH`),
/// 4. owner-only ACL on the witness.
///
/// The temporary file lives in the same directory so the rename is atomic on
/// both platforms; a crash at any point leaves either the old or the new
/// complete floor file, never a torn one.
pub fn store(path: &Path, value: u64) -> TeeResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    let encoded = encode(value);
    let mut temp_path = path.as_os_str().to_os_string();
    temp_path.push(".tmp");
    let temp_path = PathBuf::from(temp_path);

    let result = (|| -> TeeResult<()> {
        let mut file = fs::File::create(&temp_path)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        drop(file);
        replace_file_durable(&temp_path, path)?;
        crate::fs_acl::restrict_owner_only(path)?;
        Ok(())
    })();

    if result.is_err() {
        // Best-effort cleanup; the next successful store overwrites it anyway.
        let _ = fs::remove_file(&temp_path);
    }
    result
}

#[cfg(not(windows))]
fn replace_file_durable(temp_path: &Path, path: &Path) -> TeeResult<()> {
    fs::rename(temp_path, path)?;
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(windows)]
fn replace_file_durable(temp_path: &Path, path: &Path) -> TeeResult<()> {
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;
    let source: Vec<u16> = temp_path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let target: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // SAFETY: both paths are valid, NUL-terminated UTF-16 buffers for the
    // duration of the call, and the final pointer is not retained.
    let replaced = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        return Err(TeeError::IoError(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encode_decode_roundtrip() {
        for value in [0u64, 1, 7, u64::MAX] {
            assert_eq!(decode(&encode(value)).unwrap(), value);
        }
    }

    #[test]
    fn decode_rejects_truncated_floor() {
        let encoded = encode(5);
        assert!(decode(&encoded[..encoded.len() - 1]).is_err());
        assert!(decode(&[]).is_err());
    }

    #[test]
    fn decode_rejects_tampered_counter() {
        let mut encoded = encode(5);
        // Flip a counter byte without fixing the digest.
        encoded[8] ^= 0xFF;
        assert!(decode(&encoded).is_err());
    }

    #[test]
    fn decode_rejects_wrong_magic_version_and_reserved() {
        let mut encoded = encode(5);
        encoded[0] = b'X';
        assert!(decode(&encoded).is_err());

        let mut encoded = encode(5);
        encoded[4] = 2;
        assert!(decode(&encoded).is_err());

        let mut encoded = encode(5);
        encoded[6] = 1;
        assert!(decode(&encoded).is_err());
    }

    #[test]
    fn load_returns_none_for_missing_and_some_for_stored() {
        let dir = tempdir().unwrap();
        let path = floor_path(dir.path());
        assert_eq!(load(&path).unwrap(), None);

        store(&path, 9).unwrap();
        assert_eq!(load(&path).unwrap(), Some(9));

        // Overwrite must atomically replace (no leftover tmp file).
        store(&path, 10).unwrap();
        assert_eq!(load(&path).unwrap(), Some(10));
        assert!(!dir.path().join(format!("{FLOOR_FILE_NAME}.tmp")).exists());
    }

    #[test]
    fn load_fails_closed_on_corrupt_witness() {
        let dir = tempdir().unwrap();
        let path = floor_path(dir.path());
        std::fs::write(&path, b"garbage").unwrap();
        let err = load(&path).expect_err("corrupt floor must fail closed");
        assert!(
            err.to_string().contains("corrupt"),
            "unexpected error: {err}"
        );
    }
}
