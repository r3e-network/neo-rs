//! Immutable manifest generations for the append-frame prototype.
//!
//! A manifest is the commit authority for one generation of the derived
//! index: it lists every live index run (level, epoch range, file name,
//! record count, records checksum). Publication is atomic — write a `.tmp`
//! file, sync, rename over the generation's final name, then sync the
//! directory — so readers either see the complete previous generation or the
//! complete next one, never a torn manifest. Superseded manifests stay on
//! disk until explicit garbage collection, which is what lets pinned snapshot
//! generations keep their run files readable.
//!
//! Manifest entries are validated to cover every epoch from zero to the
//! generation's maximum exactly once, and each entry must name the canonical
//! run file for its level and epoch range. Format v2 authenticates the header
//! with SHA-256 in addition to the existing entry-section checksum and binds
//! the embedded generation to the immutable manifest file name. The reader
//! retains format-v1 compatibility for already-created shadow checkpoints.

use crate::engine::failpoint;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MANIFEST_MAGIC: &[u8; 8] = b"N3MANI01";
const LEGACY_MANIFEST_FORMAT_VERSION: u32 = 1;
/// Manifest format emitted by new publications; readers also accept v1.
pub const PACK_MANIFEST_FORMAT_VERSION: u32 = 2;
const LEGACY_MANIFEST_HEADER_LEN: usize = 64;
const MANIFEST_HEADER_LEN: usize = 96;
const MANIFEST_ENTRY_LEN: usize = 128;
/// Fixed on-disk field for one run file name (NUL-padded).
const MANIFEST_NAME_BYTES: usize = 56;

/// One live index run referenced by a manifest generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManifestEntry {
    pub(crate) level: u32,
    pub(crate) min_epoch: u64,
    pub(crate) max_epoch: u64,
    pub(crate) record_count: u64,
    pub(crate) records_sha256: [u8; 32],
    pub(crate) file_name: String,
}

/// One immutable generation of the derived index.
#[derive(Clone, Debug)]
pub(crate) struct Manifest {
    pub(crate) generation: u64,
    pub(crate) entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// Highest committed epoch covered by this generation.
    pub(crate) fn max_epoch(&self) -> u64 {
        self.entries
            .last()
            .expect("manifest entries are non-empty")
            .max_epoch
    }

    /// Validates the structural invariants every generation must keep:
    /// non-empty, epoch ranges contiguous from zero, level-0 runs cover
    /// exactly one epoch, and file names stay inside the runs directory.
    fn validate(&self) -> Result<()> {
        ensure!(!self.entries.is_empty(), "manifest has no entries");
        let mut expected_min = 0u64;
        for entry in &self.entries {
            ensure!(
                entry.min_epoch == expected_min,
                "manifest epoch ranges are not contiguous from zero"
            );
            ensure!(
                entry.min_epoch <= entry.max_epoch,
                "manifest entry has an inverted epoch range"
            );
            ensure!(
                entry.level > 0 || entry.min_epoch == entry.max_epoch,
                "level-0 manifest entry spans more than one epoch"
            );
            ensure!(entry.record_count > 0, "manifest entry has no records");
            validate_file_name(&entry.file_name)?;
            ensure!(
                entry.file_name == run_file_name(entry.level, entry.min_epoch, entry.max_epoch),
                "manifest entry does not name its canonical run identity"
            );
            expected_min = entry
                .max_epoch
                .checked_add(1)
                .context("manifest epoch overflows")?;
        }
        Ok(())
    }
}

/// Canonical immutable-run identity encoded by every manifest entry.
pub(crate) fn run_file_name(level: u32, min_epoch: u64, max_epoch: u64) -> String {
    if level == 0 {
        format!("run-{min_epoch:020}.idx")
    } else {
        format!("run-l{level}-{min_epoch:020}-{max_epoch:020}.idx")
    }
}

/// File name charset guard: manifest-driven path joins must never escape
/// the runs directory, even through a corrupt manifest.
fn validate_file_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty() && name.len() <= MANIFEST_NAME_BYTES,
        "manifest run file name has an invalid length"
    );
    ensure!(
        name.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.'),
        "manifest run file name has invalid characters"
    );
    Ok(())
}

/// Encodes one manifest: a 96-byte v2 header followed by fixed 128-byte
/// entries. The header carries both the SHA-256 of the entry section and a
/// SHA-256 over all preceding header fields.
pub(crate) fn encode_manifest(manifest: &Manifest) -> Result<Vec<u8>> {
    manifest.validate()?;
    let mut entries = Vec::with_capacity(manifest.entries.len() * MANIFEST_ENTRY_LEN);
    for entry in &manifest.entries {
        entries.extend_from_slice(&entry.level.to_le_bytes());
        entries.extend_from_slice(
            &u32::try_from(entry.file_name.len())
                .context("manifest run file name does not fit u32")?
                .to_le_bytes(),
        );
        entries.extend_from_slice(&entry.min_epoch.to_le_bytes());
        entries.extend_from_slice(&entry.max_epoch.to_le_bytes());
        entries.extend_from_slice(&entry.record_count.to_le_bytes());
        entries.extend_from_slice(&entry.records_sha256);
        let mut name = [0u8; MANIFEST_NAME_BYTES];
        name[..entry.file_name.len()].copy_from_slice(entry.file_name.as_bytes());
        entries.extend_from_slice(&name);
        entries.extend_from_slice(&[0u8; 8]);
    }
    ensure!(
        entries.len() == manifest.entries.len() * MANIFEST_ENTRY_LEN,
        "manifest entry encoding length changed unexpectedly"
    );
    let mut header = [0u8; MANIFEST_HEADER_LEN];
    header[0..8].copy_from_slice(MANIFEST_MAGIC);
    header[8..12].copy_from_slice(&PACK_MANIFEST_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(MANIFEST_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&manifest.generation.to_le_bytes());
    header[24..28].copy_from_slice(
        &u32::try_from(manifest.entries.len())
            .context("manifest entry count does not fit u32")?
            .to_le_bytes(),
    );
    header[32..64].copy_from_slice(&digest(&entries));
    let header_checksum = digest(&header[..64]);
    header[64..96].copy_from_slice(&header_checksum);
    let mut output = Vec::with_capacity(MANIFEST_HEADER_LEN + entries.len());
    output.extend_from_slice(&header);
    output.extend_from_slice(&entries);
    Ok(output)
}

/// Reads and fully validates one manifest file.
pub(crate) fn read_manifest(path: &Path) -> Result<Manifest> {
    let bytes = fs::read(path).with_context(|| format!("read manifest {}", path.display()))?;
    ensure!(
        bytes.len() >= LEGACY_MANIFEST_HEADER_LEN,
        "short manifest {}",
        path.display()
    );
    let prefix = &bytes[..LEGACY_MANIFEST_HEADER_LEN];
    ensure!(
        &prefix[0..8] == MANIFEST_MAGIC,
        "invalid manifest magic in {}",
        path.display()
    );
    let version = u32_at(prefix, 8)?;
    let expected_header_len = match version {
        LEGACY_MANIFEST_FORMAT_VERSION => LEGACY_MANIFEST_HEADER_LEN,
        PACK_MANIFEST_FORMAT_VERSION => MANIFEST_HEADER_LEN,
        _ => anyhow::bail!("unsupported manifest version"),
    };
    ensure!(
        u32_at(prefix, 12)? as usize == expected_header_len,
        "invalid manifest header length"
    );
    let header = bytes
        .get(..expected_header_len)
        .context("short manifest header")?;
    ensure!(
        header[28..32].iter().all(|byte| *byte == 0),
        "manifest header reserved bytes are non-zero"
    );
    if version == PACK_MANIFEST_FORMAT_VERSION {
        ensure!(
            digest(&header[..64]).as_slice() == &header[64..96],
            "manifest header checksum mismatch in {}",
            path.display()
        );
    }
    let generation = u64_at(header, 16)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("manifest path has no UTF-8 file name")?;
    ensure!(
        parse_manifest_file_name(file_name) == Some(generation),
        "manifest file name does not match embedded generation"
    );
    let entry_count = usize::try_from(u32_at(header, 24)?).context("manifest count overflows")?;
    ensure!(entry_count > 0, "empty manifest {}", path.display());
    let entries_bytes = bytes
        .get(expected_header_len..)
        .context("short manifest entries")?;
    ensure!(
        entries_bytes.len() == entry_count * MANIFEST_ENTRY_LEN,
        "manifest length mismatch in {}",
        path.display()
    );
    ensure!(
        digest(entries_bytes).as_slice() == &header[32..64],
        "manifest entries checksum mismatch in {}",
        path.display()
    );
    let mut entries = Vec::with_capacity(entry_count);
    for raw in entries_bytes.chunks_exact(MANIFEST_ENTRY_LEN) {
        let level = u32_at(raw, 0)?;
        let name_len = usize::try_from(u32_at(raw, 4)?).context("manifest name overflows")?;
        ensure!(
            (1..=MANIFEST_NAME_BYTES).contains(&name_len),
            "invalid manifest run file name length"
        );
        ensure!(
            raw[64 + name_len..64 + MANIFEST_NAME_BYTES]
                .iter()
                .all(|byte| *byte == 0),
            "manifest run file name is not NUL-padded"
        );
        ensure!(
            raw[120..128].iter().all(|byte| *byte == 0),
            "manifest entry reserved bytes are non-zero"
        );
        let file_name = std::str::from_utf8(&raw[64..64 + name_len])
            .context("manifest run file name is not UTF-8")?
            .to_owned();
        entries.push(ManifestEntry {
            level,
            min_epoch: u64_at(raw, 8)?,
            max_epoch: u64_at(raw, 16)?,
            record_count: u64_at(raw, 24)?,
            records_sha256: raw[32..64].try_into().expect("records checksum"),
            file_name,
        });
    }
    let manifest = Manifest {
        generation,
        entries,
    };
    manifest.validate()?;
    Ok(manifest)
}

/// Atomically publishes one manifest generation: write `.tmp`, sync, rename,
/// sync the directory. The rename is the single atomic publication point.
pub(crate) fn publish_manifest(root: &Path, manifest: &Manifest) -> Result<PathBuf> {
    let bytes = encode_manifest(manifest)?;
    let final_path = root.join(manifest_file_name(manifest.generation));
    let temp_path = root.join(format!("manifest-{:020}.tmp", manifest.generation));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .with_context(|| format!("create manifest {}", temp_path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write manifest {}", temp_path.display()))?;
    failpoint::crash("compaction.manifest.before-sync");
    file.sync_all()
        .with_context(|| format!("sync manifest {}", temp_path.display()))?;
    failpoint::crash("compaction.manifest.after-sync");
    drop(file);
    fs::rename(&temp_path, &final_path).with_context(|| {
        format!(
            "publish manifest {} as {}",
            temp_path.display(),
            final_path.display()
        )
    })?;
    failpoint::crash("compaction.manifest.after-rename");
    File::open(root)
        .with_context(|| format!("open directory {} for sync", root.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", root.display()))?;
    Ok(final_path)
}

pub(crate) fn manifest_file_name(generation: u64) -> String {
    format!("manifest-{generation:020}.man")
}

/// Every manifest file in `root`, newest generation first. Generations are
/// recovered from file names, so even unparseable manifests count here.
pub(crate) fn list_manifest_files(root: &Path) -> Result<Vec<(u64, PathBuf)>> {
    let mut manifests = Vec::new();
    let directory = match fs::read_dir(root) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(manifests),
        Err(error) => {
            return Err(error).with_context(|| format!("read directory {}", root.display()));
        }
    };
    for entry in directory {
        let entry = entry.context("read directory entry")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(generation) = parse_manifest_file_name(name) else {
            continue;
        };
        manifests.push((generation, entry.path()));
    }
    manifests.sort_by_key(|(generation, _)| std::cmp::Reverse(*generation));
    Ok(manifests)
}

fn parse_manifest_file_name(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("manifest-")?;
    let digits = rest.strip_suffix(".man")?;
    if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset.checked_add(4).context("u32 offset overflows")?;
    let raw: [u8; 4] = bytes
        .get(offset..end)
        .context("short u32 field")?
        .try_into()
        .expect("four-byte slice");
    Ok(u32::from_le_bytes(raw))
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64> {
    let end = offset.checked_add(8).context("u64 offset overflows")?;
    let raw: [u8; 8] = bytes
        .get(offset..end)
        .context("short u64 field")?
        .try_into()
        .expect("eight-byte slice");
    Ok(u64::from_le_bytes(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(level: u32, min_epoch: u64, max_epoch: u64, name: &str) -> ManifestEntry {
        ManifestEntry {
            level,
            min_epoch,
            max_epoch,
            record_count: 1,
            records_sha256: [7u8; 32],
            file_name: name.to_owned(),
        }
    }

    #[test]
    fn manifest_roundtrips_and_detects_corruption() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 3,
            entries: vec![
                entry(
                    1,
                    0,
                    8,
                    "run-l1-00000000000000000000-00000000000000000008.idx",
                ),
                entry(0, 9, 9, "run-00000000000000000009.idx"),
            ],
        };
        let path = publish_manifest(root.path(), &manifest).expect("publish manifest");
        let decoded = read_manifest(&path).expect("read published manifest");
        assert_eq!(decoded.generation, 3);
        assert_eq!(decoded.entries, manifest.entries);
        assert_eq!(decoded.max_epoch(), 9);
        assert_eq!(
            list_manifest_files(root.path()).expect("list manifests"),
            vec![(3, path.clone())]
        );

        let mut bytes = fs::read(&path).expect("read manifest bytes");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&path, bytes).expect("corrupt manifest");
        let error = read_manifest(&path).expect_err("corrupt manifest must fail");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn manifest_v2_binds_header_and_file_generation() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 7,
            entries: vec![entry(0, 0, 0, "run-00000000000000000000.idx")],
        };
        let path = publish_manifest(root.path(), &manifest).expect("publish manifest");
        let original = fs::read(&path).expect("read manifest bytes");
        assert_eq!(
            u32_at(&original, 8).expect("manifest version"),
            PACK_MANIFEST_FORMAT_VERSION
        );

        let mut corrupt_header = original.clone();
        corrupt_header[16] ^= 0x01;
        fs::write(&path, corrupt_header).expect("corrupt manifest header");
        let error = read_manifest(&path).expect_err("header corruption must fail");
        assert!(error.to_string().contains("header checksum mismatch"));

        fs::write(&path, &original).expect("restore manifest");
        let renamed = root.path().join(manifest_file_name(8));
        fs::rename(&path, &renamed).expect("rename manifest generation");
        let error = read_manifest(&renamed).expect_err("renamed generation must fail");
        assert!(error.to_string().contains("embedded generation"));
    }

    #[test]
    fn legacy_v1_manifest_remains_readable() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 11,
            entries: vec![entry(0, 0, 0, "run-00000000000000000000.idx")],
        };
        let v2 = encode_manifest(&manifest).expect("encode v2 manifest");
        let mut legacy = Vec::with_capacity(
            LEGACY_MANIFEST_HEADER_LEN + v2.len().saturating_sub(MANIFEST_HEADER_LEN),
        );
        legacy.extend_from_slice(&v2[..LEGACY_MANIFEST_HEADER_LEN]);
        legacy[8..12].copy_from_slice(&LEGACY_MANIFEST_FORMAT_VERSION.to_le_bytes());
        legacy[12..16].copy_from_slice(&(LEGACY_MANIFEST_HEADER_LEN as u32).to_le_bytes());
        legacy.extend_from_slice(&v2[MANIFEST_HEADER_LEN..]);
        let path = root.path().join(manifest_file_name(manifest.generation));
        fs::write(&path, legacy).expect("write legacy manifest");

        let decoded = read_manifest(&path).expect("read legacy manifest");
        assert_eq!(decoded.generation, manifest.generation);
        assert_eq!(decoded.entries, manifest.entries);
    }

    #[test]
    fn manifest_rejects_non_contiguous_epochs_and_bad_names() {
        let mut manifest = Manifest {
            generation: 1,
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 2, 2, "run-00000000000000000002.idx"),
            ],
        };
        assert!(encode_manifest(&manifest).is_err());
        manifest.entries[1].min_epoch = 1;
        manifest.entries[1].max_epoch = 2;
        assert!(encode_manifest(&manifest).is_err());
        manifest.entries[1].level = 1;
        manifest.entries[1].file_name = run_file_name(1, 1, 2);
        assert!(encode_manifest(&manifest).is_ok());
        manifest.entries[1].file_name = "run-alias.idx".to_owned();
        let error = encode_manifest(&manifest).expect_err("non-canonical run name must fail");
        assert!(error.to_string().contains("canonical run identity"));
        manifest.entries[1].file_name = "../escape.idx".to_owned();
        assert!(encode_manifest(&manifest).is_err());
    }
}
