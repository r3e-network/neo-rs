use anyhow::{Context, Result, bail};
use neo_rpc::application_logs::DEFAULT_ACCEPTED_BYTE_CAP;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(super) const ACC_INDEX_MAGIC: &[u8; 8] = b"NEOACCIX";
pub(super) const ACC_INDEX_HEADER_LEN: u64 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct IndexedResumePosition {
    pub(super) first_item: u32,
    pub(super) skipped: u64,
    pub(super) offset: u64,
}

pub(super) fn indexed_resume_position(
    acc_path: &Path,
    declared_start: u32,
    declared_count: u32,
    local_height: u32,
) -> Result<Option<IndexedResumePosition>> {
    let Some(resume_height) = local_height.checked_add(1) else {
        return Ok(None);
    };
    let Some(declared_end) = declared_start.checked_add(declared_count) else {
        bail!("declared ACC range overflow");
    };
    if resume_height < declared_start || resume_height >= declared_end {
        return Ok(None);
    }

    let index_path = acc_path.with_file_name(format!(
        "{}.offsets",
        acc_path
            .file_name()
            .and_then(|name| name.to_str())
            .context("ACC path has no UTF-8 file name")?
    ));
    if !index_path.is_file() {
        return Ok(None);
    }

    let acc_metadata = acc_path.metadata().context("failed to stat ACC file")?;
    let modified_ns = acc_metadata
        .modified()
        .context("failed to read ACC modification time")?
        .duration_since(std::time::UNIX_EPOCH)
        .context("ACC modification time precedes Unix epoch")?
        .as_nanos();
    let modified_ns = u64::try_from(modified_ns).context("ACC modification time overflow")?;
    let index_metadata = index_path
        .metadata()
        .context("failed to stat ACC offset index")?;
    let expected_index_len = ACC_INDEX_HEADER_LEN
        .checked_add(u64::from(declared_count) * 8)
        .context("ACC offset index length overflow")?;
    if index_metadata.len() != expected_index_len {
        bail!(
            "invalid ACC offset index length: expected {expected_index_len}, got {}",
            index_metadata.len()
        );
    }

    let mut index = File::open(&index_path).context("failed to open ACC offset index")?;
    let mut header = [0u8; ACC_INDEX_HEADER_LEN as usize];
    index
        .read_exact(&mut header)
        .context("failed to read ACC offset index header")?;
    let read_u32 = |offset: usize| {
        u32::from_le_bytes(
            header[offset..offset + 4]
                .try_into()
                .expect("four-byte field"),
        )
    };
    let read_u64 = |offset: usize| {
        u64::from_le_bytes(
            header[offset..offset + 8]
                .try_into()
                .expect("eight-byte field"),
        )
    };
    if &header[..8] != ACC_INDEX_MAGIC
        || read_u32(8) != declared_start
        || read_u32(12) != declared_count
        || read_u64(16) != acc_metadata.len()
        || read_u64(24) != modified_ns
    {
        bail!(
            "stale or invalid ACC offset index for {}; delete it to rebuild",
            acc_path.display()
        );
    }

    let first_item = resume_height - declared_start;
    index
        .seek(SeekFrom::Start(
            ACC_INDEX_HEADER_LEN + u64::from(first_item) * 8,
        ))
        .context("failed to seek within ACC offset index")?;
    let mut offset_bytes = [0u8; 8];
    index
        .read_exact(&mut offset_bytes)
        .context("failed to read ACC block offset")?;
    let offset = u64::from_le_bytes(offset_bytes);
    if offset < 8 || offset.saturating_add(4) > acc_metadata.len() {
        bail!("ACC offset index contains an out-of-range block offset");
    }

    Ok(Some(IndexedResumePosition {
        first_item,
        skipped: u64::from(first_item),
        offset,
    }))
}

pub(super) fn import_stop_height_from_env() -> Option<u32> {
    std::env::var("NEO_IMPORT_STOP_HEIGHT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
}

pub(super) fn import_progress_interval_from_env() -> u64 {
    std::env::var("NEO_IMPORT_PROGRESS_INTERVAL")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1_000)
}

pub(super) fn import_progress_heartbeat_secs_from_env() -> u64 {
    std::env::var("NEO_IMPORT_PROGRESS_HEARTBEAT_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(60)
}

/// Returns how many consecutive transaction-free blocks one write-back carries.
///
/// `0` and `1` both mean "persist every block on its own", which is the
/// historical behaviour and the default. A value of two or more coalesces runs
/// of that length; anything above what the persistence layer accepts is clamped
/// by [`clamp_empty_batch_size`] before it reaches a write-back.
pub(super) fn import_empty_batch_size_from_env() -> usize {
    std::env::var("NEO_IMPORT_EMPTY_BATCH_SIZE")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

pub(super) fn import_flush_interval_from_env() -> u64 {
    std::env::var("NEO_IMPORT_FLUSH_INTERVAL")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(10_000)
}

/// Returns whether a one-time full-range compaction must run before importing.
///
/// Resuming an import on a database written by a build that suppressed
/// background compaction leaves a large level-0 backlog behind. Draining it
/// once up front is much cheaper than paying the read amplification on every
/// subsequent block, so it is available as an explicit opt-in maintenance step.
pub(super) fn import_compact_on_start_from_env() -> bool {
    std::env::var("NEO_IMPORT_COMPACT_ON_START")
        .ok()
        .map(|raw| {
            let value = raw.trim();
            value.eq_ignore_ascii_case("1")
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Returns whether persistence-stage profiling was requested for this import.
///
/// Stage timings are opt-in because every sampled stage reads two clocks around
/// work the pipeline would otherwise perform untouched. When enabled, the
/// import starts its own sample so the reported deltas describe exactly this
/// run instead of the process lifetime.
pub(super) fn import_persistence_profile_from_env() -> bool {
    std::env::var("NEO_IMPORT_PERSISTENCE_PROFILE")
        .ok()
        .map(|raw| {
            let value = raw.trim();
            value.eq_ignore_ascii_case("1")
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}
/// Returns the accepted-record byte cap an import session uses.
///
/// `NEO_IMPORT_LOGS_BYTE_CAP` bounds how much application-log data one session
/// accumulates between two checkpoints. Zero commits after every finished unit,
/// which is the historical per-block write pattern; the default is
/// [`DEFAULT_ACCEPTED_BYTE_CAP`].
pub(super) fn import_logs_byte_cap_from_env() -> u64 {
    std::env::var("NEO_IMPORT_LOGS_BYTE_CAP")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_ACCEPTED_BYTE_CAP)
}
