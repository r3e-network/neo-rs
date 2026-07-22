//! # neo-state-packs manifest generations
//!
//! ## Boundary
//!
//! This module owns the durable commit description for the derived index. It
//! authenticates the selected frame extents and every immutable run identity;
//! recovery remains responsible for proving those identities against the
//! bytes currently present in the pack and run directories.
//!
//! ## Contents
//!
//! - current-only manifest-v3 encoding and decoding;
//! - the canonical frame-history chain used to bind a manifest to its pack;
//! - atomic manifest publication and generation discovery.

use crate::engine::failpoint;
use crate::engine::store::{
    PACK_INDEX_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackFrameReceipt, PackSegmentId,
    PackStoreArtifact, PackStoreConfig, PackStoreError,
};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MANIFEST_MAGIC: &[u8; 8] = b"N3MANI01";
/// Manifest format emitted and accepted by the production reader.
pub const PACK_MANIFEST_FORMAT_VERSION: u32 = 3;
const SUPPORTED_MANIFEST_FORMAT_VERSIONS: &[u32] = &[PACK_MANIFEST_FORMAT_VERSION];
const MANIFEST_HEADER_LEN: usize = 160;
const MANIFEST_EXTENT_LEN: usize = 96;
const MANIFEST_ENTRY_LEN: usize = 192;
pub(crate) const HARD_MAX_MANIFEST_EXTENTS: usize = PackStoreConfig::HARD_MAX_RECENT_RUNS;
const HARD_MAX_MANIFEST_ENTRIES: usize = PackStoreConfig::HARD_MAX_RECENT_RUNS;
/// Fixed on-disk field for one run file name (NUL-padded).
const MANIFEST_NAME_BYTES: usize = 56;

const MANIFEST_HEADER_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/manifest-header/v3\0";
const MANIFEST_EXTENTS_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/manifest-extents/v3\0";
const MANIFEST_ENTRIES_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/manifest-entries/v3\0";
const FRAME_CHAIN_SEED_DOMAIN: &[u8] = b"neo-state-packs/frame-chain-seed/v3\0";
const FRAME_CHAIN_STEP_DOMAIN: &[u8] = b"neo-state-packs/frame-chain-step/v3\0";

/// One authenticated contiguous extent of one immutable frame segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ManifestExtent {
    pub(crate) segment_id: PackSegmentId,
    pub(crate) first_epoch: u64,
    pub(crate) frame_count: u64,
    pub(crate) frame_start: u64,
    pub(crate) frame_end: u64,
    pub(crate) frame_chain_sha256: [u8; 32],
}

impl ManifestExtent {
    pub(crate) fn last_epoch(self) -> Result<u64> {
        self.first_epoch
            .checked_add(self.frame_count.saturating_sub(1))
            .context("manifest extent epoch overflows")
    }
}

/// One live index run referenced by a manifest generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManifestEntry {
    pub(crate) level: u32,
    pub(crate) min_epoch: u64,
    pub(crate) max_epoch: u64,
    pub(crate) format_version: u32,
    pub(crate) record_count: u64,
    pub(crate) records_offset: u64,
    pub(crate) file_bytes: u64,
    pub(crate) records_sha256: [u8; 32],
    pub(crate) structure_sha256: [u8; 32],
    pub(crate) file_name: String,
}

/// One immutable generation of the derived index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    pub(crate) generation: u64,
    pub(crate) extents: Vec<ManifestExtent>,
    pub(crate) entries: Vec<ManifestEntry>,
}

impl Manifest {
    /// Number of frames selected by this generation.
    pub(crate) fn frame_count(&self) -> Result<u64> {
        self.extents.iter().try_fold(0u64, |total, extent| {
            total
                .checked_add(extent.frame_count)
                .context("manifest frame count overflows")
        })
    }

    /// Validates the structural invariants every generation must keep.
    fn validate(&self) -> Result<()> {
        ensure!(!self.extents.is_empty(), "manifest has no segment extents");
        ensure!(!self.entries.is_empty(), "manifest has no entries");
        ensure!(
            self.extents.len() <= HARD_MAX_MANIFEST_EXTENTS,
            "manifest extent count exceeds the format hard limit"
        );
        ensure!(
            self.entries.len() <= HARD_MAX_MANIFEST_ENTRIES,
            "manifest entry count exceeds the format hard limit"
        );
        ensure!(
            self.extents[0].segment_id == PackSegmentId::INITIAL,
            "manifest segment extents do not start at segment zero"
        );

        let mut expected_epoch = 0u64;
        let mut previous_segment: Option<PackSegmentId> = None;
        for extent in &self.extents {
            ensure!(extent.frame_count > 0, "manifest extent has no frames");
            ensure!(
                extent.frame_start == PACK_SEGMENT_HEADER_LEN,
                "manifest segment extent does not begin after its segment header"
            );
            ensure!(
                extent.frame_start < extent.frame_end,
                "manifest extent is empty"
            );
            if let Some(previous) = previous_segment {
                ensure!(
                    previous.checked_next() == Some(extent.segment_id),
                    "manifest segment extents are not consecutive"
                );
            }
            ensure!(
                extent.first_epoch == expected_epoch,
                "manifest segment epochs are not contiguous"
            );
            expected_epoch = extent
                .last_epoch()?
                .checked_add(1)
                .context("manifest extent epoch overflows")?;
            previous_segment = Some(extent.segment_id);
        }

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
            if entry.format_version != PACK_INDEX_FORMAT_VERSION {
                return Err(PackStoreError::unsupported_version(
                    PackStoreArtifact::IndexRun,
                    entry.format_version,
                    &[PACK_INDEX_FORMAT_VERSION],
                )
                .into());
            }
            ensure!(
                entry.records_offset < entry.file_bytes,
                "manifest entry has an invalid run extent"
            );
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
        ensure!(
            expected_min == expected_epoch,
            "manifest runs do not cover the selected frame extents"
        );
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

/// Returns whether `name` is an exact immutable-run name emitted by this
/// engine. Cleanup paths use this classifier so unrelated files are never
/// removed merely because they share an extension.
pub(crate) fn is_run_file_name(name: &str) -> bool {
    let Some(body) = name
        .strip_prefix("run-")
        .and_then(|name| name.strip_suffix(".idx"))
    else {
        return false;
    };
    if let Some(body) = body.strip_prefix('l') {
        let mut fields = body.split('-');
        let (Some(level), Some(min_epoch), Some(max_epoch), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return false;
        };
        let (Some(level), Some(min_epoch), Some(max_epoch)) = (
            level.parse::<u32>().ok().filter(|level| *level > 0),
            parse_fixed_u64(min_epoch),
            parse_fixed_u64(max_epoch),
        ) else {
            return false;
        };
        min_epoch <= max_epoch && run_file_name(level, min_epoch, max_epoch) == name
    } else {
        parse_fixed_u64(body).is_some_and(|epoch| run_file_name(0, epoch, epoch) == name)
    }
}

/// Returns whether `name` is an exact temporary-run name emitted by append,
/// recovery, or compaction publication.
pub(crate) fn is_run_temp_file_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".tmp") else {
        return false;
    };
    if is_run_file_name(stem) {
        return true;
    }
    stem.strip_prefix("run-")
        .and_then(parse_fixed_u64)
        .is_some_and(|epoch| stem == format!("run-{epoch:020}"))
}

/// Extends the authenticated frame extent set with one next frame receipt.
pub(crate) fn append_frame_extent(
    extents: &mut Vec<ManifestExtent>,
    receipt: PackFrameReceipt,
) -> Result<()> {
    ensure!(
        receipt.frame_start < receipt.frame_end,
        "frame receipt has an empty extent"
    );
    if let Some(last) = extents.last_mut()
        && last.segment_id == receipt.segment_id
    {
        let expected_epoch = last
            .first_epoch
            .checked_add(last.frame_count)
            .context("manifest frame epoch overflows")?;
        ensure!(
            receipt.epoch == expected_epoch,
            "frame receipt is not next in its manifest extent"
        );
        ensure!(
            receipt.frame_start == last.frame_end,
            "frame receipt does not continue its manifest extent"
        );
        last.frame_count = last
            .frame_count
            .checked_add(1)
            .context("manifest frame count overflows")?;
        last.frame_end = receipt.frame_end;
        last.frame_chain_sha256 = frame_chain_step(last.frame_chain_sha256, receipt);
        return Ok(());
    }

    ensure!(
        receipt.frame_start == PACK_SEGMENT_HEADER_LEN,
        "first frame in a manifest segment does not begin after its segment header"
    );
    if let Some(last) = extents.last() {
        ensure!(
            last.segment_id.checked_next() == Some(receipt.segment_id),
            "frame receipt segment is not next in its manifest extent set"
        );
        ensure!(
            last.last_epoch()?.checked_add(1) == Some(receipt.epoch),
            "frame receipt epoch is not next after the previous segment"
        );
    } else {
        ensure!(
            receipt.segment_id == PackSegmentId::INITIAL,
            "first frame receipt does not belong to segment zero"
        );
        ensure!(
            receipt.epoch == 0,
            "first frame receipt does not start at epoch zero"
        );
    }
    extents.push(ManifestExtent {
        segment_id: receipt.segment_id,
        first_epoch: receipt.epoch,
        frame_count: 1,
        frame_start: receipt.frame_start,
        frame_end: receipt.frame_end,
        frame_chain_sha256: frame_chain_step(
            frame_chain_seed(receipt.segment_id, receipt.epoch),
            receipt,
        ),
    });
    Ok(())
}

/// Starts a domain-separated frame-history chain for one segment extent.
pub(crate) fn frame_chain_seed(segment_id: PackSegmentId, first_epoch: u64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(FRAME_CHAIN_SEED_DOMAIN);
    hasher.update(segment_id.get().to_le_bytes());
    hasher.update(first_epoch.to_le_bytes());
    hasher.finalize().into()
}

/// Adds one canonical receipt to a frame-history chain.
pub(crate) fn frame_chain_step(previous: [u8; 32], receipt: PackFrameReceipt) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(FRAME_CHAIN_STEP_DOMAIN);
    hasher.update(previous);
    encode_receipt(&mut hasher, receipt);
    hasher.finalize().into()
}

fn encode_receipt(hasher: &mut Sha256, receipt: PackFrameReceipt) {
    hasher.update(receipt.epoch.to_le_bytes());
    hasher.update(receipt.segment_id.get().to_le_bytes());
    hasher.update(receipt.frame_start.to_le_bytes());
    hasher.update(receipt.frame_end.to_le_bytes());
    hasher.update(receipt.context.block_start.to_le_bytes());
    hasher.update(receipt.context.block_end.to_le_bytes());
    hasher.update(receipt.context.previous_root);
    hasher.update(receipt.context.resulting_root);
    hasher.update(receipt.rows.to_le_bytes());
    hasher.update(receipt.metadata_bytes.to_le_bytes());
    hasher.update(receipt.value_bytes.to_le_bytes());
    hasher.update(receipt.frame_sha256);
}

/// File name charset guard: manifest-driven path joins must never escape the
/// runs directory, even through a corrupt manifest.
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

/// Encodes one current-only manifest-v3 generation.
pub(crate) fn encode_manifest(manifest: &Manifest) -> Result<Vec<u8>> {
    manifest.validate()?;

    let extent_count =
        u32::try_from(manifest.extents.len()).context("manifest extent count does not fit u32")?;
    let entry_count =
        u32::try_from(manifest.entries.len()).context("manifest entry count does not fit u32")?;
    let extents_capacity = manifest
        .extents
        .len()
        .checked_mul(MANIFEST_EXTENT_LEN)
        .context("manifest extent encoding length overflows")?;
    let entries_capacity = manifest
        .entries
        .len()
        .checked_mul(MANIFEST_ENTRY_LEN)
        .context("manifest entry encoding length overflows")?;
    let mut extents = Vec::new();
    extents
        .try_reserve_exact(extents_capacity)
        .context("reserve manifest extent encoding")?;
    for extent in &manifest.extents {
        extents.extend_from_slice(&extent.segment_id.get().to_le_bytes());
        extents.extend_from_slice(&extent.first_epoch.to_le_bytes());
        extents.extend_from_slice(&extent.frame_count.to_le_bytes());
        extents.extend_from_slice(&extent.frame_start.to_le_bytes());
        extents.extend_from_slice(&extent.frame_end.to_le_bytes());
        extents.extend_from_slice(&extent.frame_chain_sha256);
        extents.extend_from_slice(&[0u8; 24]);
    }

    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entries_capacity)
        .context("reserve manifest entry encoding")?;
    for entry in &manifest.entries {
        entries.extend_from_slice(&entry.level.to_le_bytes());
        entries.extend_from_slice(&entry.format_version.to_le_bytes());
        entries.extend_from_slice(&entry.min_epoch.to_le_bytes());
        entries.extend_from_slice(&entry.max_epoch.to_le_bytes());
        entries.extend_from_slice(&entry.record_count.to_le_bytes());
        entries.extend_from_slice(&entry.records_offset.to_le_bytes());
        entries.extend_from_slice(&entry.file_bytes.to_le_bytes());
        entries.extend_from_slice(&entry.records_sha256);
        entries.extend_from_slice(&entry.structure_sha256);
        entries.extend_from_slice(
            &u32::try_from(entry.file_name.len())
                .context("manifest run file name does not fit u32")?
                .to_le_bytes(),
        );
        entries.extend_from_slice(&[0u8; 4]);
        let mut name = [0u8; MANIFEST_NAME_BYTES];
        name[..entry.file_name.len()].copy_from_slice(entry.file_name.as_bytes());
        entries.extend_from_slice(&name);
        entries.extend_from_slice(&[0u8; 16]);
    }
    ensure!(
        extents.len() == extents_capacity,
        "manifest extent encoding length changed unexpectedly"
    );
    ensure!(
        entries.len() == entries_capacity,
        "manifest entry encoding length changed unexpectedly"
    );

    let output_len = MANIFEST_HEADER_LEN
        .checked_add(extents.len())
        .and_then(|length| length.checked_add(entries.len()))
        .context("manifest encoding length overflows")?;

    let mut header = [0u8; MANIFEST_HEADER_LEN];
    header[0..8].copy_from_slice(MANIFEST_MAGIC);
    header[8..12].copy_from_slice(&PACK_MANIFEST_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(MANIFEST_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&manifest.generation.to_le_bytes());
    header[24..28].copy_from_slice(&extent_count.to_le_bytes());
    header[28..32].copy_from_slice(&entry_count.to_le_bytes());
    header[32..40].copy_from_slice(&manifest.frame_count()?.to_le_bytes());
    header[40..72].copy_from_slice(&section_digest(MANIFEST_EXTENTS_DIGEST_DOMAIN, &extents));
    header[72..104].copy_from_slice(&section_digest(MANIFEST_ENTRIES_DIGEST_DOMAIN, &entries));
    let header_digest = manifest_header_digest(&header);
    header[104..136].copy_from_slice(&header_digest);

    let mut output = Vec::new();
    output
        .try_reserve_exact(output_len)
        .context("reserve manifest encoding")?;
    output.extend_from_slice(&header);
    output.extend_from_slice(&extents);
    output.extend_from_slice(&entries);
    Ok(output)
}

/// Reads and fully validates one current-only manifest file.
pub(crate) fn read_manifest(path: &Path) -> Result<Manifest> {
    let mut file = File::open(path).with_context(|| format!("open manifest {}", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("stat manifest {}", path.display()))?
        .len();
    ensure!(file_len >= 16, "short manifest {}", path.display());

    // Read only the fixed prefix before accepting any attacker-controlled
    // count. This lets the current-only version/header checks fail without
    // allocating for an untrusted body.
    let mut prefix = [0u8; 16];
    file.read_exact(&mut prefix)
        .with_context(|| format!("read manifest prefix {}", path.display()))?;
    ensure!(
        &prefix[0..8] == MANIFEST_MAGIC,
        "invalid manifest magic in {}",
        path.display()
    );
    let version = u32_at(&prefix, 8)?;
    if version != PACK_MANIFEST_FORMAT_VERSION {
        return Err(PackStoreError::UnsupportedVersion {
            artifact: PackStoreArtifact::Manifest,
            found: version,
            supported: SUPPORTED_MANIFEST_FORMAT_VERSIONS,
        }
        .into());
    }
    ensure!(
        u32_at(&prefix, 12)? as usize == MANIFEST_HEADER_LEN,
        "invalid manifest header length"
    );
    ensure!(
        file_len >= MANIFEST_HEADER_LEN as u64,
        "short manifest header {}",
        path.display()
    );
    let mut header = [0u8; MANIFEST_HEADER_LEN];
    header[..prefix.len()].copy_from_slice(&prefix);
    file.read_exact(&mut header[prefix.len()..])
        .with_context(|| format!("read manifest header {}", path.display()))?;
    ensure!(
        header[136..].iter().all(|byte| *byte == 0),
        "manifest header reserved bytes are non-zero"
    );
    ensure!(
        manifest_header_digest(&header).as_slice() == &header[104..136],
        "manifest header checksum mismatch in {}",
        path.display()
    );

    let generation = u64_at(&header, 16)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("manifest path has no UTF-8 file name")?;
    ensure!(
        parse_manifest_file_name(file_name) == Some(generation),
        "manifest file name does not match embedded generation"
    );
    let extent_count =
        usize::try_from(u32_at(&header, 24)?).context("manifest extent count overflows")?;
    let entry_count = usize::try_from(u32_at(&header, 28)?).context("manifest count overflows")?;
    ensure!(extent_count > 0, "manifest has no segment extents");
    ensure!(entry_count > 0, "manifest has no entries");
    ensure!(
        extent_count <= HARD_MAX_MANIFEST_EXTENTS,
        "manifest extent count exceeds the format hard limit"
    );
    ensure!(
        entry_count <= HARD_MAX_MANIFEST_ENTRIES,
        "manifest entry count exceeds the format hard limit"
    );
    let extents_len = extent_count
        .checked_mul(MANIFEST_EXTENT_LEN)
        .context("manifest extent length overflows")?;
    let entries_len = entry_count
        .checked_mul(MANIFEST_ENTRY_LEN)
        .context("manifest entry length overflows")?;
    let extents_start = MANIFEST_HEADER_LEN;
    let entries_start = extents_start
        .checked_add(extents_len)
        .context("manifest entries offset overflows")?;
    let expected_len = entries_start
        .checked_add(entries_len)
        .context("manifest length overflows")?;
    ensure!(
        file_len == u64::try_from(expected_len).context("manifest length does not fit u64")?,
        "manifest length mismatch in {}",
        path.display()
    );
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(expected_len)
        .context("reserve manifest bytes")?;
    bytes.extend_from_slice(&header);
    bytes.resize(expected_len, 0);
    file.read_exact(&mut bytes[MANIFEST_HEADER_LEN..])
        .with_context(|| format!("read manifest body {}", path.display()))?;
    let mut trailing = [0u8; 1];
    ensure!(
        file.read(&mut trailing)
            .with_context(|| format!("check manifest length {}", path.display()))?
            == 0,
        "manifest grew while reading {}",
        path.display()
    );
    let extents_bytes = &bytes[extents_start..entries_start];
    let entries_bytes = &bytes[entries_start..];
    ensure!(
        section_digest(MANIFEST_EXTENTS_DIGEST_DOMAIN, extents_bytes).as_slice() == &header[40..72],
        "manifest extents checksum mismatch in {}",
        path.display()
    );
    ensure!(
        section_digest(MANIFEST_ENTRIES_DIGEST_DOMAIN, entries_bytes).as_slice()
            == &header[72..104],
        "manifest entries checksum mismatch in {}",
        path.display()
    );

    let mut extents = Vec::new();
    extents
        .try_reserve_exact(extent_count)
        .context("reserve manifest extents")?;
    for raw in extents_bytes.chunks_exact(MANIFEST_EXTENT_LEN) {
        ensure!(
            raw[72..].iter().all(|byte| *byte == 0),
            "manifest extent reserved bytes are non-zero"
        );
        extents.push(ManifestExtent {
            segment_id: PackSegmentId::new(u64_at(raw, 0)?),
            first_epoch: u64_at(raw, 8)?,
            frame_count: u64_at(raw, 16)?,
            frame_start: u64_at(raw, 24)?,
            frame_end: u64_at(raw, 32)?,
            frame_chain_sha256: raw[40..72].try_into().expect("frame chain checksum"),
        });
    }

    let mut entries = Vec::new();
    entries
        .try_reserve_exact(entry_count)
        .context("reserve manifest entries")?;
    for raw in entries_bytes.chunks_exact(MANIFEST_ENTRY_LEN) {
        let name_len = usize::try_from(u32_at(raw, 112)?).context("manifest name overflows")?;
        ensure!(
            (1..=MANIFEST_NAME_BYTES).contains(&name_len),
            "invalid manifest run file name length"
        );
        ensure!(
            raw[116..120].iter().all(|byte| *byte == 0) && raw[176..].iter().all(|byte| *byte == 0),
            "manifest entry reserved bytes are non-zero"
        );
        ensure!(
            raw[120 + name_len..120 + MANIFEST_NAME_BYTES]
                .iter()
                .all(|byte| *byte == 0),
            "manifest run file name is not NUL-padded"
        );
        let name = std::str::from_utf8(&raw[120..120 + name_len])
            .context("manifest run file name is not UTF-8")?;
        let mut file_name = String::new();
        file_name
            .try_reserve_exact(name_len)
            .context("reserve manifest run file name")?;
        file_name.push_str(name);
        entries.push(ManifestEntry {
            level: u32_at(raw, 0)?,
            format_version: u32_at(raw, 4)?,
            min_epoch: u64_at(raw, 8)?,
            max_epoch: u64_at(raw, 16)?,
            record_count: u64_at(raw, 24)?,
            records_offset: u64_at(raw, 32)?,
            file_bytes: u64_at(raw, 40)?,
            records_sha256: raw[48..80].try_into().expect("records checksum"),
            structure_sha256: raw[80..112].try_into().expect("structure checksum"),
            file_name,
        });
    }
    let manifest = Manifest {
        generation,
        extents,
        entries,
    };
    ensure!(
        manifest.frame_count()? == u64_at(&header, 32)?,
        "manifest frame count does not match its extents"
    );
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

/// Finds the newest manifest generation in one directory scan without
/// retaining manifest history in memory.
pub(crate) fn newest_manifest_file(root: &Path) -> Result<Option<(u64, PathBuf)>> {
    let directory = match fs::read_dir(root) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| format!("read directory {}", root.display()));
        }
    };
    let mut newest: Option<(u64, PathBuf)> = None;
    for entry in directory {
        let entry = entry.context("read directory entry")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let Some(generation) = parse_manifest_file_name(name) else {
            continue;
        };
        if newest
            .as_ref()
            .is_none_or(|(current, _)| generation > *current)
        {
            newest = Some((generation, entry.path()));
        }
    }
    Ok(newest)
}

/// Every manifest file in `root`, newest generation first. Generations are
/// recovered from file names, so even unparseable manifests count here.
#[cfg(test)]
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

pub(crate) fn parse_manifest_file_name(name: &str) -> Option<u64> {
    let rest = name.strip_prefix("manifest-")?;
    let digits = rest.strip_suffix(".man")?;
    if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Returns whether `name` is an exact temporary manifest name emitted by this
/// engine.
pub(crate) fn is_manifest_temp_file_name(name: &str) -> bool {
    let Some(digits) = name
        .strip_prefix("manifest-")
        .and_then(|name| name.strip_suffix(".tmp"))
    else {
        return false;
    };
    parse_fixed_u64(digits)
        .is_some_and(|generation| name == format!("manifest-{generation:020}.tmp"))
}

fn parse_fixed_u64(digits: &str) -> Option<u64> {
    if digits.len() != 20 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn section_digest(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    hasher.finalize().into()
}

fn manifest_header_digest(header: &[u8; MANIFEST_HEADER_LEN]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(MANIFEST_HEADER_DIGEST_DOMAIN);
    hasher.update(&header[..104]);
    hasher.update(&header[136..]);
    hasher.finalize().into()
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

    fn receipt(epoch: u64, end: u64) -> PackFrameReceipt {
        PackFrameReceipt {
            epoch,
            segment_id: PackSegmentId::INITIAL,
            frame_start: end - 10,
            frame_end: end,
            context: crate::engine::store::PackFrameContext::new(
                epoch as u32,
                epoch as u32,
                [epoch as u8; 32],
                [epoch as u8 + 1; 32],
            ),
            rows: 1,
            metadata_bytes: 56,
            value_bytes: 0,
            frame_sha256: [epoch as u8 + 2; 32],
        }
    }

    fn extent_set() -> Vec<ManifestExtent> {
        let mut extents = Vec::new();
        append_frame_extent(&mut extents, receipt(0, 74)).expect("first extent");
        append_frame_extent(
            &mut extents,
            PackFrameReceipt {
                frame_start: 74,
                frame_end: 84,
                ..receipt(1, 84)
            },
        )
        .expect("second extent");
        extents
    }

    fn two_segment_extent_set() -> Vec<ManifestExtent> {
        let mut extents = Vec::new();
        append_frame_extent(&mut extents, receipt(0, 74)).expect("first segment extent");
        append_frame_extent(
            &mut extents,
            PackFrameReceipt {
                segment_id: PackSegmentId::new(1),
                frame_start: PACK_SEGMENT_HEADER_LEN,
                frame_end: PACK_SEGMENT_HEADER_LEN + 10,
                ..receipt(1, 74)
            },
        )
        .expect("second segment extent");
        extents
    }

    fn entry(level: u32, min_epoch: u64, max_epoch: u64, name: &str) -> ManifestEntry {
        ManifestEntry {
            level,
            min_epoch,
            max_epoch,
            format_version: PACK_INDEX_FORMAT_VERSION,
            record_count: 1,
            records_offset: 192,
            file_bytes: 242,
            records_sha256: [7u8; 32],
            structure_sha256: [8u8; 32],
            file_name: name.to_owned(),
        }
    }

    #[test]
    fn manifest_roundtrips_and_detects_corruption() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 3,
            extents: extent_set(),
            entries: vec![
                entry(
                    1,
                    0,
                    0,
                    "run-l1-00000000000000000000-00000000000000000000.idx",
                ),
                entry(0, 1, 1, "run-00000000000000000001.idx"),
            ],
        };
        let path = publish_manifest(root.path(), &manifest).expect("publish manifest");
        let decoded = read_manifest(&path).expect("read published manifest");
        assert_eq!(decoded, manifest);
        assert_eq!(decoded.entries.last().expect("last entry").max_epoch, 1);
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
    fn manifest_v3_binds_header_and_file_generation() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 7,
            extents: extent_set(),
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 1, 1, "run-00000000000000000001.idx"),
            ],
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
    fn unknown_manifest_version_is_rejected() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 9,
            extents: extent_set(),
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 1, 1, "run-00000000000000000001.idx"),
            ],
        };
        let path = publish_manifest(root.path(), &manifest).expect("publish manifest");
        let mut bytes = fs::read(&path).expect("read manifest");
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        fs::write(&path, bytes).expect("write unknown version");
        let error = read_manifest(&path).expect_err("unknown manifest version must fail");
        assert!(
            error
                .downcast_ref::<PackStoreError>()
                .is_some_and(|error| matches!(
                    error,
                    PackStoreError::UnsupportedVersion { found: 99, .. }
                ))
        );
    }

    #[test]
    fn unknown_manifest_version_is_rejected_from_prefix_before_sparse_body_read() {
        let root = tempdir().expect("temporary manifest root");
        let path = root.path().join(manifest_file_name(12));
        let mut file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .expect("create sparse manifest");
        let mut prefix = [0u8; 16];
        prefix[..8].copy_from_slice(MANIFEST_MAGIC);
        prefix[8..12].copy_from_slice(&99u32.to_le_bytes());
        prefix[12..16].copy_from_slice(&(MANIFEST_HEADER_LEN as u32).to_le_bytes());
        file.write_all(&prefix).expect("write manifest prefix");
        file.set_len(u64::from(u32::MAX) + 1)
            .expect("extend sparse manifest");
        drop(file);

        let error = read_manifest(&path).expect_err("unknown manifest version must fail");
        assert!(matches!(
            error.downcast_ref::<PackStoreError>(),
            Some(PackStoreError::UnsupportedVersion {
                artifact: PackStoreArtifact::Manifest,
                found: 99,
                ..
            })
        ));
    }

    #[test]
    fn current_manifest_counts_above_hard_limits_fail_before_body_read() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 14,
            extents: extent_set(),
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 1, 1, "run-00000000000000000001.idx"),
            ],
        };
        let path = publish_manifest(root.path(), &manifest).expect("publish manifest");
        let original = fs::read(&path).expect("read manifest");
        let excessive =
            u32::try_from(HARD_MAX_MANIFEST_ENTRIES + 1).expect("manifest hard limit fits u32");

        for (count_offset, extents, entries, expected_error) in [
            (
                24,
                usize::try_from(excessive).expect("extent count fits usize"),
                manifest.entries.len(),
                "manifest extent count exceeds the format hard limit",
            ),
            (
                28,
                manifest.extents.len(),
                usize::try_from(excessive).expect("entry count fits usize"),
                "manifest entry count exceeds the format hard limit",
            ),
        ] {
            let mut header: [u8; MANIFEST_HEADER_LEN] = original[..MANIFEST_HEADER_LEN]
                .try_into()
                .expect("manifest header");
            header[count_offset..count_offset + 4].copy_from_slice(&excessive.to_le_bytes());
            let checksum = manifest_header_digest(&header);
            header[104..136].copy_from_slice(&checksum);
            fs::write(&path, header).expect("write oversized manifest header");
            let sparse_len = MANIFEST_HEADER_LEN
                .checked_add(
                    extents
                        .checked_mul(MANIFEST_EXTENT_LEN)
                        .expect("sparse extent bytes"),
                )
                .and_then(|length| {
                    entries
                        .checked_mul(MANIFEST_ENTRY_LEN)
                        .and_then(|bytes| length.checked_add(bytes))
                })
                .expect("sparse manifest length");
            OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("open sparse manifest")
                .set_len(u64::try_from(sparse_len).expect("sparse length fits u64"))
                .expect("extend sparse manifest");

            let error = read_manifest(&path).expect_err("oversized current manifest must fail");
            assert!(error.to_string().contains(expected_error));
        }
    }

    #[test]
    fn manifest_reader_rejects_trailing_bytes() {
        let root = tempdir().expect("temporary manifest root");
        let manifest = Manifest {
            generation: 13,
            extents: extent_set(),
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 1, 1, "run-00000000000000000001.idx"),
            ],
        };
        let path = publish_manifest(root.path(), &manifest).expect("publish manifest");
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open manifest for append")
            .write_all(&[0])
            .expect("append trailing byte");

        let error = read_manifest(&path).expect_err("trailing manifest byte must fail");
        assert!(error.to_string().contains("length mismatch"));
    }

    #[test]
    fn newest_manifest_selection_scans_large_unordered_history() {
        let root = tempdir().expect("temporary manifest history");
        const GENERATIONS: u64 = 4_096;
        for generation in (1..=GENERATIONS).rev() {
            File::create(root.path().join(manifest_file_name(generation)))
                .expect("create manifest history entry");
        }
        File::create(root.path().join("manifest-not-a-generation.man"))
            .expect("create unrelated manifest-like file");

        let newest = newest_manifest_file(root.path())
            .expect("scan manifest history")
            .expect("newest manifest");
        assert_eq!(newest.0, GENERATIONS);
        assert_eq!(
            newest.1.file_name().and_then(|name| name.to_str()),
            Some(manifest_file_name(GENERATIONS).as_str())
        );
    }

    #[test]
    fn frame_extent_rejects_empty_following_receipt_without_mutation() {
        let mut extents = Vec::new();
        append_frame_extent(&mut extents, receipt(0, 74)).expect("first extent");
        let original = extents.clone();

        for frame_end in [74, 73] {
            let error = append_frame_extent(
                &mut extents,
                PackFrameReceipt {
                    frame_start: 74,
                    frame_end,
                    ..receipt(1, 84)
                },
            )
            .expect_err("empty or reversed next frame must fail");
            assert!(error.to_string().contains("empty extent"));
            assert_eq!(extents, original);
        }
    }

    #[test]
    fn frame_extent_builder_requires_segment_zero_and_header_aligned_starts() {
        for invalid in [
            PackFrameReceipt {
                segment_id: PackSegmentId::new(1),
                ..receipt(0, 74)
            },
            PackFrameReceipt {
                frame_start: PACK_SEGMENT_HEADER_LEN + 1,
                frame_end: PACK_SEGMENT_HEADER_LEN + 11,
                ..receipt(0, 74)
            },
        ] {
            let mut extents = Vec::new();
            assert!(append_frame_extent(&mut extents, invalid).is_err());
            assert!(extents.is_empty());
        }

        let mut extents = extent_set();
        let original = extents.clone();
        let invalid_rotation = PackFrameReceipt {
            segment_id: PackSegmentId::new(1),
            frame_start: PACK_SEGMENT_HEADER_LEN + 1,
            frame_end: PACK_SEGMENT_HEADER_LEN + 11,
            ..receipt(2, 84)
        };
        assert!(append_frame_extent(&mut extents, invalid_rotation).is_err());
        assert_eq!(extents, original);
    }

    #[test]
    fn manifest_rejects_nonzero_origin_and_non_header_extent_start() {
        let mut manifest = Manifest {
            generation: 15,
            extents: two_segment_extent_set(),
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 1, 1, "run-00000000000000000001.idx"),
            ],
        };

        manifest.extents[0].segment_id = PackSegmentId::new(1);
        let error = encode_manifest(&manifest).expect_err("nonzero origin must fail");
        assert!(error.to_string().contains("segment zero"));

        manifest.extents = two_segment_extent_set();
        manifest.extents[1].frame_start += 1;
        let error = encode_manifest(&manifest).expect_err("misaligned segment extent must fail");
        assert!(error.to_string().contains("segment header"));
    }

    #[test]
    fn manifest_rejects_non_contiguous_epochs_and_bad_names() {
        let mut manifest = Manifest {
            generation: 1,
            extents: extent_set(),
            entries: vec![
                entry(0, 0, 0, "run-00000000000000000000.idx"),
                entry(0, 2, 2, "run-00000000000000000002.idx"),
            ],
        };
        assert!(encode_manifest(&manifest).is_err());
        manifest.entries[1].min_epoch = 1;
        manifest.entries[1].max_epoch = 1;
        manifest.entries[1].file_name = run_file_name(0, 1, 1);
        assert!(encode_manifest(&manifest).is_ok());
        manifest.entries[1].file_name = "run-alias.idx".to_owned();
        let error = encode_manifest(&manifest).expect_err("non-canonical run name must fail");
        assert!(error.to_string().contains("canonical run identity"));
        manifest.entries[1].file_name = "../escape.idx".to_owned();
        assert!(encode_manifest(&manifest).is_err());
    }
}
