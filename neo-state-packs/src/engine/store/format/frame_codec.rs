//! Version-2 append-frame encoding and authentication.
//!
//! Frames separate fixed-width row metadata from packed values so index
//! rebuilds can scan keys without decoding or copying value bytes. The fixed
//! header authenticates both sections independently; the footer binds the
//! header digest to the epoch and exact complete-frame length.

use super::segment::{SEGMENT_HEADER_LEN, segment_path};
use super::*;

pub(super) const FRAME_METADATA_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/frame-v2/metadata\0";
pub(super) const FRAME_VALUE_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/frame-v2/values\0";
const FRAME_CONTENT_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/frame-v2/content\0";
const FRAME_FOOTER_DIGEST_DOMAIN: &[u8] = b"neo-state-packs/frame-v2/footer\0";
const HEADER_RESERVED_START: usize = 196;
const RECEIPT_HASH_BUFFER_BYTES: usize = 64 * 1024;
const PAYLOAD_PROGRESS_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FrameHeader {
    pub(super) epoch: u64,
    pub(super) context: PackFrameContext,
    pub(super) rows: u64,
    pub(super) metadata_bytes: u64,
    pub(super) value_bytes: u64,
    pub(super) frame_bytes: u64,
    pub(super) metadata_sha256: [u8; 32],
    pub(super) value_sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PendingFrameRow {
    pub(super) key: [u8; PACK_KEY_BYTES],
    pub(super) sequence: u32,
    pub(super) value_offset: u64,
    pub(super) value_len: u32,
    pub(super) tombstone: bool,
}

/// Encodes owned operations into canonical metadata and value sections.
///
/// Rows are sorted by key and original sequence. Values are copied once, in
/// that final metadata order, and index offsets remain relative until the
/// publication path knows the frame's physical start.
pub(super) fn encode_frame_payload(
    operations: &[PackOperation],
) -> Result<(Vec<u8>, Vec<u8>, Vec<IndexEntry>)> {
    ensure!(
        !operations.is_empty(),
        "frame must contain at least one row"
    );
    let rows = operations.len();
    let rows_u64 = u64::try_from(rows).context("frame row count overflows u64")?;
    ensure!(
        rows_u64 <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    ensure!(
        rows <= u32::MAX as usize,
        "frame sequence space exceeds u32"
    );

    let metadata_bytes = rows
        .checked_mul(FRAME_ROW_METADATA_LEN)
        .context("frame metadata size overflows usize")?;
    let mut value_bytes = 0usize;
    for operation in operations {
        ensure!(
            operation.key[0] == FRAME_NODE_KEY_PREFIX,
            "frame key is outside the MPT node namespace"
        );
        if let PackOpKind::Put(value) = &operation.kind {
            let _ = u32::try_from(value.len()).context("frame value exceeds u32")?;
            value_bytes = value_bytes
                .checked_add(value.len())
                .context("frame value size overflows usize")?;
        }
    }
    validate_payload_bounds(rows_u64, metadata_bytes as u64, value_bytes as u64)?;

    let mut order = Vec::new();
    order
        .try_reserve_exact(rows)
        .context("reserve frame order")?;
    order.extend(0..rows);
    order.sort_unstable_by(|left, right| {
        operations[*left]
            .key
            .cmp(&operations[*right].key)
            .then_with(|| left.cmp(right))
    });

    let mut values = Vec::new();
    values
        .try_reserve_exact(value_bytes)
        .context("reserve frame values")?;
    let mut pending = Vec::new();
    pending
        .try_reserve_exact(rows)
        .context("reserve frame metadata rows")?;
    for index in order {
        let operation = &operations[index];
        let sequence = u32::try_from(index).context("frame sequence exceeds u32")?;
        match &operation.kind {
            PackOpKind::Put(value) => {
                let value_offset =
                    u64::try_from(values.len()).context("frame value offset overflows u64")?;
                let value_len = u32::try_from(value.len()).context("frame value exceeds u32")?;
                values.extend_from_slice(value);
                pending.push(PendingFrameRow {
                    key: operation.key,
                    sequence,
                    value_offset,
                    value_len,
                    tombstone: false,
                });
            }
            PackOpKind::Tombstone => pending.push(PendingFrameRow {
                key: operation.key,
                sequence,
                value_offset: 0,
                value_len: 0,
                tombstone: true,
            }),
        }
    }
    encode_pending_rows(pending, values)
}

/// Serializes already-sorted pending rows into the final frame sections.
pub(super) fn encode_pending_rows(
    rows: Vec<PendingFrameRow>,
    values: Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>, Vec<IndexEntry>)> {
    ensure!(!rows.is_empty(), "frame must contain at least one row");
    let row_count = rows.len();
    let row_count_u64 = u64::try_from(row_count).context("frame row count overflows u64")?;
    let metadata_len = row_count
        .checked_mul(FRAME_ROW_METADATA_LEN)
        .context("frame metadata size overflows usize")?;
    validate_payload_bounds(row_count_u64, metadata_len as u64, values.len() as u64)?;

    let mut seen_sequences = sequence_bitmap(row_count)?;
    let mut metadata = Vec::new();
    metadata
        .try_reserve_exact(metadata_len)
        .context("reserve encoded frame metadata")?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(row_count)
        .context("reserve frame index entries")?;
    let mut previous: Option<([u8; PACK_KEY_BYTES], u32)> = None;
    let mut value_cursor = 0u64;

    for row in rows {
        ensure!(
            row.key[0] == FRAME_NODE_KEY_PREFIX,
            "frame key is outside the MPT node namespace"
        );
        let order = (row.key, row.sequence);
        if let Some(previous) = previous {
            ensure!(
                previous < order,
                "frame metadata rows are not ordered by key and sequence"
            );
        }
        previous = Some(order);
        let sequence = usize::try_from(row.sequence).context("frame sequence overflows usize")?;
        ensure!(
            sequence < row_count,
            "frame sequence is outside its row set"
        );
        ensure!(
            !seen_sequences[sequence],
            "frame contains a duplicate sequence"
        );
        seen_sequences[sequence] = true;

        if row.tombstone {
            ensure!(
                row.value_offset == 0 && row.value_len == 0,
                "tombstone carries a non-zero value location"
            );
        } else {
            ensure!(
                row.value_offset == value_cursor,
                "put values are not contiguous in metadata order"
            );
            value_cursor = value_cursor
                .checked_add(u64::from(row.value_len))
                .context("frame value range overflows")?;
            ensure!(
                value_cursor <= values.len() as u64,
                "frame metadata value range exceeds its value section"
            );
        }

        metadata.extend_from_slice(&row.key[1..]);
        metadata.push(u8::from(!row.tombstone));
        metadata.extend_from_slice(&[0; 3]);
        metadata.extend_from_slice(&row.sequence.to_le_bytes());
        metadata.extend_from_slice(&row.value_offset.to_le_bytes());
        metadata.extend_from_slice(&row.value_len.to_le_bytes());
        metadata.extend_from_slice(&[0; 4]);
        entries.push(IndexEntry {
            key: row.key,
            sequence: row.sequence,
            value_offset: row.value_offset,
            value_len: row.value_len,
            tombstone: row.tombstone,
        });
    }
    ensure!(
        value_cursor == values.len() as u64,
        "frame value section contains unreferenced bytes"
    );
    ensure!(
        metadata.len() == metadata_len,
        "encoded frame metadata length changed unexpectedly"
    );
    Ok((metadata, values, entries))
}

pub(super) fn encode_frame_header(
    epoch: u64,
    context: PackFrameContext,
    rows: usize,
    metadata_len: usize,
    value_len: usize,
    metadata_sha256: [u8; 32],
    value_sha256: [u8; 32],
) -> Result<[u8; FRAME_HEADER_LEN]> {
    validate_frame_context(context)?;
    let rows = u64::try_from(rows).context("frame row count does not fit u64")?;
    let metadata_bytes = u64::try_from(metadata_len).context("metadata length does not fit u64")?;
    let value_bytes = u64::try_from(value_len).context("value length does not fit u64")?;
    let frame_bytes = validate_payload_bounds(rows, metadata_bytes, value_bytes)?;

    let mut header = [0u8; FRAME_HEADER_LEN];
    header[0..8].copy_from_slice(FRAME_MAGIC);
    header[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&epoch.to_le_bytes());
    header[24..28].copy_from_slice(&context.block_start.to_le_bytes());
    header[28..32].copy_from_slice(&context.block_end.to_le_bytes());
    header[32..40].copy_from_slice(&rows.to_le_bytes());
    header[40..48].copy_from_slice(&metadata_bytes.to_le_bytes());
    header[48..56].copy_from_slice(&value_bytes.to_le_bytes());
    header[56..64].copy_from_slice(&frame_bytes.to_le_bytes());
    header[64..96].copy_from_slice(&context.previous_root);
    header[96..128].copy_from_slice(&context.resulting_root);
    header[128..160].copy_from_slice(&metadata_sha256);
    header[160..192].copy_from_slice(&value_sha256);
    header[192..196].copy_from_slice(&(FRAME_FOOTER_LEN as u32).to_le_bytes());
    debug_assert!(
        header[HEADER_RESERVED_START..]
            .iter()
            .all(|byte| *byte == 0)
    );
    Ok(header)
}

/// Validates a fixed frame header without allocating from persisted lengths.
pub(super) fn validate_frame_header(
    header: &[u8; FRAME_HEADER_LEN],
    expected_epoch: u64,
) -> Result<FrameHeader> {
    validate_frame_magic(header)?;
    let version = u32_at(header, 8)?;
    if version != PACK_FRAME_FORMAT_VERSION {
        return Err(PackStoreError::unsupported_version(
            PackStoreArtifact::Frame,
            version,
            &[PACK_FRAME_FORMAT_VERSION],
        )
        .into());
    }
    ensure!(
        u32_at(header, 12)? as usize == FRAME_HEADER_LEN,
        "invalid frame header length"
    );
    let epoch = u64_at(header, 16)?;
    ensure!(epoch == expected_epoch, "non-contiguous frame epoch");
    let context = PackFrameContext {
        block_start: u32_at(header, 24)?,
        block_end: u32_at(header, 28)?,
        previous_root: header[64..96].try_into().expect("previous-root field"),
        resulting_root: header[96..128].try_into().expect("resulting-root field"),
    };
    validate_frame_context(context)?;
    let rows = u64_at(header, 32)?;
    let metadata_bytes = u64_at(header, 40)?;
    let value_bytes = u64_at(header, 48)?;
    let frame_bytes = validate_payload_bounds(rows, metadata_bytes, value_bytes)?;
    ensure!(
        u64_at(header, 56)? == frame_bytes,
        "frame header declares an inconsistent complete length"
    );
    ensure!(
        u32_at(header, 192)? as usize == FRAME_FOOTER_LEN,
        "invalid frame footer length"
    );
    ensure!(
        header[HEADER_RESERVED_START..]
            .iter()
            .all(|byte| *byte == 0),
        "frame header reserved bytes are non-zero"
    );
    Ok(FrameHeader {
        epoch,
        context,
        rows,
        metadata_bytes,
        value_bytes,
        frame_bytes,
        metadata_sha256: header[128..160]
            .try_into()
            .expect("metadata checksum field"),
        value_sha256: header[160..192].try_into().expect("value checksum field"),
    })
}

fn validate_frame_magic(header: &[u8; FRAME_HEADER_LEN]) -> Result<()> {
    if &header[..8] == FRAME_MAGIC {
        return Ok(());
    }
    if let Some(version) = frame_magic_version(&header[..8]) {
        return Err(PackStoreError::unsupported_version(
            PackStoreArtifact::Frame,
            version,
            &[PACK_FRAME_FORMAT_VERSION],
        )
        .into());
    }
    anyhow::bail!("invalid frame magic")
}

fn frame_magic_version(magic: &[u8]) -> Option<u32> {
    if magic.len() != 8
        || &magic[..6] != b"N3PACK"
        || !magic[6].is_ascii_digit()
        || !magic[7].is_ascii_digit()
    {
        return None;
    }
    Some(u32::from(magic[6] - b'0') * 10 + u32::from(magic[7] - b'0'))
}

pub(super) fn frame_metadata_digest(metadata: &[u8]) -> [u8; 32] {
    section_digest(FRAME_METADATA_DIGEST_DOMAIN, metadata)
}

pub(super) fn frame_value_digest(values: &[u8]) -> [u8; 32] {
    section_digest(FRAME_VALUE_DIGEST_DOMAIN, values)
}

pub(super) fn frame_digest(header: &[u8; FRAME_HEADER_LEN]) -> [u8; 32] {
    section_digest(FRAME_CONTENT_DIGEST_DOMAIN, header)
}

fn section_digest(domain: &[u8], bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    hasher.update(bytes);
    hasher.finalize().into()
}

pub(super) fn encode_frame_footer(
    epoch: u64,
    frame_bytes: u64,
    frame_sha256: [u8; 32],
) -> Result<[u8; FRAME_FOOTER_LEN]> {
    ensure!(
        frame_bytes >= (FRAME_HEADER_LEN + FRAME_FOOTER_LEN) as u64,
        "complete frame length is too short"
    );
    let mut footer = [0u8; FRAME_FOOTER_LEN];
    footer[0..8].copy_from_slice(FRAME_FOOTER_MAGIC);
    footer[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
    footer[12..16].copy_from_slice(&(FRAME_FOOTER_LEN as u32).to_le_bytes());
    footer[16..24].copy_from_slice(&epoch.to_le_bytes());
    footer[24..32].copy_from_slice(&frame_bytes.to_le_bytes());
    footer[32..64].copy_from_slice(&frame_sha256);
    let footer_sha256 = section_digest(FRAME_FOOTER_DIGEST_DOMAIN, &footer[..64]);
    footer[64..96].copy_from_slice(&footer_sha256);
    Ok(footer)
}

pub(super) fn validate_frame_footer(
    footer: &[u8; FRAME_FOOTER_LEN],
    header: FrameHeader,
    expected_frame_sha256: [u8; 32],
) -> Result<()> {
    ensure!(
        &footer[..8] == FRAME_FOOTER_MAGIC,
        "invalid frame footer magic"
    );
    let version = u32_at(footer, 8)?;
    if version != PACK_FRAME_FORMAT_VERSION {
        return Err(PackStoreError::unsupported_version(
            PackStoreArtifact::Frame,
            version,
            &[PACK_FRAME_FORMAT_VERSION],
        )
        .into());
    }
    ensure!(
        u32_at(footer, 12)? as usize == FRAME_FOOTER_LEN,
        "invalid frame footer length"
    );
    ensure!(
        u64_at(footer, 16)? == header.epoch,
        "frame footer epoch mismatch"
    );
    ensure!(
        u64_at(footer, 24)? == header.frame_bytes,
        "frame footer length mismatch"
    );
    ensure!(
        footer[32..64] == expected_frame_sha256,
        "frame footer content digest mismatch"
    );
    ensure!(
        footer[64..96] == section_digest(FRAME_FOOTER_DIGEST_DOMAIN, &footer[..64]),
        "frame footer checksum mismatch"
    );
    Ok(())
}

fn validate_payload_bounds(rows: u64, metadata_bytes: u64, value_bytes: u64) -> Result<u64> {
    ensure!(rows > 0, "frame must contain at least one row");
    ensure!(
        rows <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let expected_metadata = rows
        .checked_mul(FRAME_ROW_METADATA_LEN as u64)
        .context("frame metadata length overflows")?;
    ensure!(
        metadata_bytes == expected_metadata,
        "frame metadata length does not match its row count"
    );
    let payload_bytes = metadata_bytes
        .checked_add(value_bytes)
        .context("frame payload length overflows")?;
    ensure!(
        payload_bytes <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    payload_bytes
        .checked_add(FRAME_HEADER_LEN as u64)
        .and_then(|bytes| bytes.checked_add(FRAME_FOOTER_LEN as u64))
        .context("complete frame length overflows")
}

/// Validated complete-frame end offsets. Bytes beyond the last end are an
/// uncommitted torn or orphan suffix handled by recovery.
pub(super) struct FrameScan {
    pub(super) frame_ends: Vec<u64>,
}

/// Performs a bounded structural scan without hashing the large sections.
///
/// Torn or malformed tails stop the scan. A numeric `N3PACKxx` version that
/// this binary does not support is propagated as a typed failure so an older
/// binary cannot silently truncate a newer store.
pub(super) fn scan_frames(pack: &File) -> Result<FrameScan> {
    let file_len = pack.metadata().context("stat append pack")?.len();
    ensure!(
        file_len >= SEGMENT_HEADER_LEN as u64,
        "append pack is shorter than its segment header"
    );
    let mut frame_ends = Vec::new();
    let mut offset = SEGMENT_HEADER_LEN as u64;
    let mut expected_epoch = 0u64;
    while offset < file_len {
        let mut header_bytes = [0u8; FRAME_HEADER_LEN];
        let header_bytes_read = read_available(pack, offset, &mut header_bytes)?;
        if header_bytes_read < FRAME_HEADER_LEN {
            if header_bytes_read >= FRAME_MAGIC.len()
                && &header_bytes[..FRAME_MAGIC.len()] != FRAME_MAGIC
                && let Some(version) = frame_magic_version(&header_bytes[..FRAME_MAGIC.len()])
            {
                return Err(PackStoreError::unsupported_version(
                    PackStoreArtifact::Frame,
                    version,
                    &[PACK_FRAME_FORMAT_VERSION],
                )
                .into());
            }
            break;
        }
        let header = match validate_frame_header(&header_bytes, expected_epoch) {
            Ok(header) => header,
            Err(error) if is_unsupported_version(&error) => return Err(error),
            Err(_) => break,
        };
        let frame_end = match offset.checked_add(header.frame_bytes) {
            Some(end) if end <= file_len => end,
            _ => break,
        };
        let footer_start = frame_end - FRAME_FOOTER_LEN as u64;
        let mut footer = [0u8; FRAME_FOOTER_LEN];
        if read_available(pack, footer_start, &mut footer)? < FRAME_FOOTER_LEN {
            break;
        }
        let expected_digest = frame_digest(&header_bytes);
        match validate_frame_footer(&footer, header, expected_digest) {
            Ok(()) => {}
            Err(error) if is_unsupported_version(&error) => return Err(error),
            Err(_) => break,
        }
        frame_ends.push(frame_end);
        offset = frame_end;
        expected_epoch = expected_epoch
            .checked_add(1)
            .context("frame epoch overflows")?;
    }
    Ok(FrameScan { frame_ends })
}

fn is_unsupported_version(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<PackStoreError>()
        .is_some_and(|error| matches!(error, PackStoreError::UnsupportedVersion { .. }))
}

fn read_available(file: &File, offset: u64, bytes: &mut [u8]) -> Result<usize> {
    let mut filled = 0usize;
    while filled < bytes.len() {
        let read_offset = offset
            .checked_add(u64::try_from(filled).context("frame read offset does not fit u64")?)
            .context("frame read offset overflows")?;
        let read = file
            .read_at(&mut bytes[filled..], read_offset)
            .context("read append-frame bytes")?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    Ok(filled)
}

pub(super) fn read_frame_receipt(
    pack: &File,
    scan: &FrameScan,
    epoch: u64,
) -> Result<PackFrameReceipt> {
    let index = usize::try_from(epoch).context("frame epoch does not fit usize")?;
    let frame_end = *scan
        .frame_ends
        .get(index)
        .with_context(|| format!("frame {epoch} is not present in the append pack"))?;
    let frame_start = if index == 0 {
        SEGMENT_HEADER_LEN as u64
    } else {
        scan.frame_ends[index - 1]
    };
    read_frame_receipt_at(pack, epoch, frame_start, frame_end)
}

/// Re-authenticates a complete frame using bounded streaming section hashes.
pub(super) fn read_frame_receipt_at(
    pack: &File,
    epoch: u64,
    frame_start: u64,
    frame_end: u64,
) -> Result<PackFrameReceipt> {
    let mut header_bytes = [0u8; FRAME_HEADER_LEN];
    pack.read_exact_at(&mut header_bytes, frame_start)
        .with_context(|| format!("read frame {epoch} header"))?;
    let header = validate_frame_header(&header_bytes, epoch)?;
    ensure!(
        frame_start.checked_add(header.frame_bytes) == Some(frame_end),
        "frame {epoch} length does not match the validated frame chain"
    );
    let metadata_start = frame_start
        .checked_add(FRAME_HEADER_LEN as u64)
        .context("frame metadata offset overflows")?;
    let value_start = metadata_start
        .checked_add(header.metadata_bytes)
        .context("frame value offset overflows")?;
    let footer_start = value_start
        .checked_add(header.value_bytes)
        .context("frame footer offset overflows")?;
    ensure!(
        footer_start.checked_add(FRAME_FOOTER_LEN as u64) == Some(frame_end),
        "frame {epoch} section lengths do not reach its exact end"
    );
    ensure!(
        hash_file_section(
            pack,
            metadata_start,
            header.metadata_bytes,
            FRAME_METADATA_DIGEST_DOMAIN
        )? == header.metadata_sha256,
        "frame {epoch} metadata checksum mismatch"
    );
    ensure!(
        hash_file_section(
            pack,
            value_start,
            header.value_bytes,
            FRAME_VALUE_DIGEST_DOMAIN
        )? == header.value_sha256,
        "frame {epoch} value checksum mismatch"
    );
    let frame_sha256 = frame_digest(&header_bytes);
    let mut footer = [0u8; FRAME_FOOTER_LEN];
    pack.read_exact_at(&mut footer, footer_start)
        .with_context(|| format!("read frame {epoch} footer"))?;
    validate_frame_footer(&footer, header, frame_sha256)?;
    Ok(receipt_from_header(
        frame_start,
        frame_end,
        header,
        frame_sha256,
    ))
}

fn hash_file_section(file: &File, offset: u64, len: u64, domain: &[u8]) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(domain);
    let mut buffer = [0u8; RECEIPT_HASH_BUFFER_BYTES];
    let mut consumed = 0u64;
    while consumed < len {
        let remaining = len - consumed;
        let chunk_len = usize::try_from(remaining.min(buffer.len() as u64))
            .context("frame hash chunk does not fit usize")?;
        let read_offset = offset
            .checked_add(consumed)
            .context("authenticated frame section offset overflows")?;
        file.read_exact_at(&mut buffer[..chunk_len], read_offset)
            .context("read authenticated frame section")?;
        hasher.update(&buffer[..chunk_len]);
        consumed += chunk_len as u64;
    }
    Ok(hasher.finalize().into())
}

fn receipt_from_header(
    frame_start: u64,
    frame_end: u64,
    header: FrameHeader,
    frame_sha256: [u8; 32],
) -> PackFrameReceipt {
    PackFrameReceipt {
        epoch: header.epoch,
        segment_id: PackSegmentId::INITIAL,
        frame_start,
        frame_end,
        context: header.context,
        rows: header.rows,
        metadata_bytes: header.metadata_bytes,
        value_bytes: header.value_bytes,
        frame_sha256,
    }
}

/// Discards derived visibility state and truncates the payload stream to the
/// canonical marker. This runs only during startup, before snapshots or the
/// single writer exist, so no manifest lease can observe the reset.
pub(super) fn reset_derived_state_to_frame_prefix(
    root: &Path,
    scan: &FrameScan,
    expected_frames: u64,
) -> Result<()> {
    let expected = usize::try_from(expected_frames).context("frame count does not fit usize")?;
    let committed_end = if expected == 0 {
        SEGMENT_HEADER_LEN as u64
    } else {
        *scan
            .frame_ends
            .get(expected - 1)
            .context("committed frame prefix is incomplete")?
    };
    let pack_path = segment_path(root, PackSegmentId::INITIAL);
    let pack = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&pack_path)
        .with_context(|| format!("open append pack {} for recovery", pack_path.display()))?;
    if pack
        .metadata()
        .context("stat append pack for recovery")?
        .len()
        != committed_end
    {
        pack.set_len(committed_end)
            .context("truncate append pack to canonical marker")?;
        pack.sync_data()
            .context("sync marker-truncated append pack")?;
    }
    drop(pack);

    let runs_dir = root.join("runs");
    for entry in fs::read_dir(&runs_dir)
        .with_context(|| format!("read index-run directory {}", runs_dir.display()))?
    {
        let entry = entry.context("read index-run recovery entry")?;
        let path = entry.path();
        let remove = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension == "idx" || extension == "tmp");
        if remove {
            fs::remove_file(&path)
                .with_context(|| format!("remove derived index run {}", path.display()))?;
        }
    }
    for entry in fs::read_dir(root)
        .with_context(|| format!("read pack root {} for recovery", root.display()))?
    {
        let entry = entry.context("read pack recovery entry")?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("manifest-") && (name.ends_with(".man") || name.ends_with(".tmp")) {
            fs::remove_file(&path)
                .with_context(|| format!("remove derived manifest {}", path.display()))?;
        }
    }
    sync_directory(&runs_dir)?;
    sync_directory(root)?;
    Ok(())
}

/// Fully verifies a committed frame and returns its authenticated receipt and
/// decoded row statistics.
pub(super) fn verify_frame(
    pack: &Mmap,
    frame_start: u64,
    frame_end: u64,
    epoch: u64,
) -> Result<(PackFrameReceipt, PayloadRowStats)> {
    let start = usize::try_from(frame_start).context("frame offset does not fit usize")?;
    let end = usize::try_from(frame_end).context("frame end does not fit usize")?;
    let frame = pack
        .as_slice()
        .get(start..end)
        .context("committed frame lies outside mapped pack")?;
    validate_complete_frame(frame, epoch, frame_start, frame_end)
}

pub(super) fn validate_complete_frame(
    frame: &[u8],
    epoch: u64,
    frame_start: u64,
    frame_end: u64,
) -> Result<(PackFrameReceipt, PayloadRowStats)> {
    let header_bytes: &[u8; FRAME_HEADER_LEN] = frame
        .get(..FRAME_HEADER_LEN)
        .context("read committed frame header")?
        .try_into()
        .expect("frame header length");
    let header = validate_frame_header(header_bytes, epoch)?;
    ensure!(
        usize::try_from(header.frame_bytes).ok() == Some(frame.len()),
        "committed frame length mismatch"
    );
    ensure!(
        frame_start.checked_add(header.frame_bytes) == Some(frame_end),
        "committed frame end mismatch"
    );
    let metadata_end = FRAME_HEADER_LEN
        .checked_add(usize::try_from(header.metadata_bytes).context("metadata size too large")?)
        .context("metadata range overflows")?;
    let value_end = metadata_end
        .checked_add(usize::try_from(header.value_bytes).context("value size too large")?)
        .context("value range overflows")?;
    let metadata = frame
        .get(FRAME_HEADER_LEN..metadata_end)
        .context("committed frame metadata is truncated")?;
    let values = frame
        .get(metadata_end..value_end)
        .context("committed frame values are truncated")?;
    let footer: &[u8; FRAME_FOOTER_LEN] = frame
        .get(value_end..)
        .context("committed frame footer is truncated")?
        .try_into()
        .context("committed frame footer length mismatch")?;
    ensure!(
        frame_metadata_digest(metadata) == header.metadata_sha256,
        "frame metadata checksum mismatch in committed frame"
    );
    ensure!(
        frame_value_digest(values) == header.value_sha256,
        "frame value checksum mismatch in committed frame"
    );
    let frame_sha256 = frame_digest(header_bytes);
    validate_frame_footer(footer, header, frame_sha256)?;
    let expected_rows = usize::try_from(header.rows).context("row count does not fit usize")?;
    let stats = validate_payload_rows(metadata, values, expected_rows)?;
    Ok((
        receipt_from_header(frame_start, frame_end, header, frame_sha256),
        stats,
    ))
}

/// Decodes canonical metadata into absolute index entries without reading
/// any values.
pub(super) fn decode_frame_metadata(
    frame_start: u64,
    metadata: &[u8],
    value_bytes: u64,
) -> Result<Vec<IndexEntry>> {
    ensure!(
        metadata.len().is_multiple_of(FRAME_ROW_METADATA_LEN),
        "frame metadata has a partial row"
    );
    let expected_rows = metadata.len() / FRAME_ROW_METADATA_LEN;
    let value_start = frame_start
        .checked_add(FRAME_HEADER_LEN as u64)
        .and_then(|offset| offset.checked_add(metadata.len() as u64))
        .context("absolute frame value offset overflows")?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(expected_rows)
        .context("reserve rebuilt index entries")?;
    validate_payload_metadata_with(
        metadata,
        expected_rows,
        value_bytes,
        &mut |key, kind, sequence, relative_offset, value_len| {
            let tombstone = kind == 0;
            let value_offset = if tombstone {
                0
            } else {
                value_start
                    .checked_add(relative_offset)
                    .context("absolute frame value offset overflows")?
            };
            entries.push(IndexEntry {
                key: *key,
                sequence,
                value_offset,
                value_len,
                tombstone,
            });
            Ok(())
        },
    )?;
    Ok(entries)
}

/// Validates metadata and returns its distinct-key count without decoding
/// values or materializing index entries.
pub(super) fn scan_frame_metadata_distinct_keys(
    metadata: &[u8],
    expected_rows: usize,
    value_bytes: u64,
) -> Result<usize> {
    let mut distinct = 0usize;
    let mut previous_key: Option<[u8; PACK_KEY_BYTES]> = None;
    validate_payload_metadata_with(
        metadata,
        expected_rows,
        value_bytes,
        &mut |key, _, _, _, _| {
            if previous_key.as_ref() != Some(key) {
                distinct = distinct
                    .checked_add(1)
                    .context("distinct key count overflows")?;
                previous_key = Some(*key);
            }
            Ok(())
        },
    )?;
    Ok(distinct)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PayloadRowStats {
    pub(super) rows: u64,
    pub(super) puts: u64,
    pub(super) tombstones: u64,
    pub(super) value_bytes: u64,
}

/// Independently advancing payload section reported by streaming validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FramePayloadSection {
    Metadata,
    Values,
}

pub(super) fn validate_payload_rows(
    metadata: &[u8],
    values: &[u8],
    expected_rows: usize,
) -> Result<PayloadRowStats> {
    validate_payload_rows_with(metadata, values, expected_rows, &mut |_, _, _| Ok(()))
}

pub(super) fn validate_payload_rows_with<F>(
    metadata: &[u8],
    values: &[u8],
    expected_rows: usize,
    visit: &mut F,
) -> Result<PayloadRowStats>
where
    F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
{
    validate_payload_rows_internal(
        metadata,
        values,
        expected_rows,
        visit,
        &mut |_, _, _| Ok(()),
    )
}

/// Validates and visits rows while advancing metadata and value cursors
/// independently.
///
/// `consumed` is section-relative and monotone for each section. A callback
/// receives each contiguous slice exactly once, after every row referring to
/// it has been validated and visited, so callers may hash and release mapped
/// pages immediately.
pub(super) fn validate_payload_rows_with_progress<F, P>(
    metadata: &[u8],
    values: &[u8],
    expected_rows: usize,
    visit: &mut F,
    progress: &mut P,
) -> Result<PayloadRowStats>
where
    F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
    P: FnMut(FramePayloadSection, &[u8], usize) -> Result<()>,
{
    validate_payload_rows_internal(metadata, values, expected_rows, visit, progress)
}

fn validate_payload_rows_internal<F, P>(
    metadata: &[u8],
    values: &[u8],
    expected_rows: usize,
    visit: &mut F,
    progress: &mut P,
) -> Result<PayloadRowStats>
where
    F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
    P: FnMut(FramePayloadSection, &[u8], usize) -> Result<()>,
{
    let mut metadata_consumed = 0usize;
    let mut metadata_reported = 0usize;
    let mut values_consumed = 0usize;
    let mut values_reported = 0usize;
    let stats = validate_payload_metadata_with(
        metadata,
        expected_rows,
        values.len() as u64,
        &mut |key, kind, _, value_offset, value_len| {
            let value = if kind == 0 {
                &[][..]
            } else {
                let start = usize::try_from(value_offset).context("value offset too large")?;
                let end = start
                    .checked_add(value_len as usize)
                    .context("value range overflows")?;
                values.get(start..end).context("frame value is truncated")?
            };
            visit(key, kind, value)?;

            metadata_consumed = metadata_consumed
                .checked_add(FRAME_ROW_METADATA_LEN)
                .context("metadata progress overflows")?;
            report_complete_chunks(
                FramePayloadSection::Metadata,
                metadata,
                metadata_consumed,
                &mut metadata_reported,
                progress,
            )?;
            if kind != 0 {
                values_consumed = usize::try_from(
                    value_offset
                        .checked_add(u64::from(value_len))
                        .context("value progress overflows")?,
                )
                .context("value progress does not fit usize")?;
                report_complete_chunks(
                    FramePayloadSection::Values,
                    values,
                    values_consumed,
                    &mut values_reported,
                    progress,
                )?;
            }
            Ok(())
        },
    )?;
    report_remainder(
        FramePayloadSection::Metadata,
        metadata,
        metadata_consumed,
        &mut metadata_reported,
        progress,
    )?;
    report_remainder(
        FramePayloadSection::Values,
        values,
        values_consumed,
        &mut values_reported,
        progress,
    )?;
    Ok(stats)
}

fn report_complete_chunks<P>(
    section: FramePayloadSection,
    bytes: &[u8],
    consumed: usize,
    reported: &mut usize,
    progress: &mut P,
) -> Result<()>
where
    P: FnMut(FramePayloadSection, &[u8], usize) -> Result<()>,
{
    while consumed.saturating_sub(*reported) >= PAYLOAD_PROGRESS_BYTES {
        let end = reported
            .checked_add(PAYLOAD_PROGRESS_BYTES)
            .context("payload progress range overflows")?;
        progress(section, &bytes[*reported..end], end)?;
        *reported = end;
    }
    Ok(())
}

fn report_remainder<P>(
    section: FramePayloadSection,
    bytes: &[u8],
    consumed: usize,
    reported: &mut usize,
    progress: &mut P,
) -> Result<()>
where
    P: FnMut(FramePayloadSection, &[u8], usize) -> Result<()>,
{
    if *reported < consumed {
        progress(section, &bytes[*reported..consumed], consumed)?;
        *reported = consumed;
    }
    Ok(())
}

fn validate_payload_metadata_with<F>(
    metadata: &[u8],
    expected_rows: usize,
    value_bytes: u64,
    visit: &mut F,
) -> Result<PayloadRowStats>
where
    F: FnMut(&[u8; PACK_KEY_BYTES], u8, u32, u64, u32) -> Result<()>,
{
    ensure!(expected_rows > 0, "frame must contain at least one row");
    let rows_u64 = u64::try_from(expected_rows).context("row count does not fit u64")?;
    let expected_metadata = expected_rows
        .checked_mul(FRAME_ROW_METADATA_LEN)
        .context("frame metadata length overflows usize")?;
    validate_payload_bounds(rows_u64, expected_metadata as u64, value_bytes)?;
    ensure!(
        metadata.len() == expected_metadata,
        "frame metadata length does not match its row count"
    );

    let mut seen_sequences = sequence_bitmap(expected_rows)?;
    let mut previous: Option<([u8; PACK_KEY_BYTES], u32)> = None;
    let mut value_cursor = 0u64;
    let mut stats = PayloadRowStats::default();
    for row in metadata.chunks_exact(FRAME_ROW_METADATA_LEN) {
        let mut key = [0u8; PACK_KEY_BYTES];
        key[0] = FRAME_NODE_KEY_PREFIX;
        key[1..].copy_from_slice(&row[..32]);
        let kind = row[32];
        ensure!(kind <= 1, "invalid frame row kind");
        ensure!(
            row[33..36].iter().all(|byte| *byte == 0),
            "frame row reserved bytes are non-zero"
        );
        let sequence = u32_at(row, 36)?;
        let value_offset = u64_at(row, 40)?;
        let value_len = u32_at(row, 48)?;
        ensure!(
            row[52..56].iter().all(|byte| *byte == 0),
            "frame row reserved bytes are non-zero"
        );

        let order = (key, sequence);
        if let Some(previous) = previous {
            ensure!(
                previous < order,
                "frame metadata rows are not ordered by key and sequence"
            );
        }
        previous = Some(order);
        let sequence_index =
            usize::try_from(sequence).context("frame sequence does not fit usize")?;
        ensure!(
            sequence_index < expected_rows,
            "frame sequence is outside its row set"
        );
        ensure!(
            !seen_sequences[sequence_index],
            "frame contains a duplicate sequence"
        );
        seen_sequences[sequence_index] = true;

        if kind == 0 {
            ensure!(
                value_offset == 0 && value_len == 0,
                "tombstone carries a non-zero value location"
            );
            stats.tombstones = stats.tombstones.saturating_add(1);
        } else {
            ensure!(
                value_offset == value_cursor,
                "put values are not contiguous in metadata order"
            );
            value_cursor = value_cursor
                .checked_add(u64::from(value_len))
                .context("frame value range overflows")?;
            ensure!(
                value_cursor <= value_bytes,
                "frame value range is truncated"
            );
            stats.puts = stats.puts.saturating_add(1);
            stats.value_bytes = stats.value_bytes.saturating_add(u64::from(value_len));
        }
        visit(&key, kind, sequence, value_offset, value_len)?;
    }
    ensure!(
        value_cursor == value_bytes,
        "frame value section contains unreferenced bytes"
    );
    stats.rows = rows_u64;
    Ok(stats)
}

fn sequence_bitmap(rows: usize) -> Result<Vec<bool>> {
    let mut seen = Vec::new();
    seen.try_reserve_exact(rows)
        .context("reserve frame sequence bitmap")?;
    seen.resize(rows, false);
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN_FRAME_V2_HEX: &str = "4e335041434b303202000000e0000000070000000000000064000000690000000400000000000000e0000000000000000600000000000000260200000000000011111111111111111111111111111111111111111111111111111111111111112222222222222222222222222222222222222222222222222222222222222222557f4a6dff115d1e07bb7a3e561fdd398e88fc87e7437a29cb7fdd327fccb8a22bf99881b0c04ee7d6776ffd0bc851005ca666fda763f2fee8315228e90f6a23600000000000000000000000000000000000000000000000000000000000000001010101010101010101010101010101010101010101010101010101010101010100000001000000000000000000000000000000000000000202020202020202020202020202020202020202020202020202020202020202000000000200000000000000000000000000000000000000030303030303030303030303030303030303030303030303030303030303030301000000000000000000000000000000030000000000000003030303030303030303030303030303030303030303030303030303030303030100000003000000030000000000000003000000000000006f6c646e65774e33504b454e4432020000006000000007000000000000002602000000000000447384aaea48985e7abf91a3d80e7790f9a0303ba9d730b1536928de5e3bd64655ec6224ff8a5c200528ab8b618576f91af56689a671543a8bb023cdf299eb30";

    fn key(byte: u8) -> [u8; PACK_KEY_BYTES] {
        let mut key = [byte; PACK_KEY_BYTES];
        key[0] = FRAME_NODE_KEY_PREFIX;
        key
    }

    fn golden_parts() -> Result<(
        [u8; FRAME_HEADER_LEN],
        Vec<u8>,
        Vec<u8>,
        [u8; FRAME_FOOTER_LEN],
    )> {
        let operations = vec![
            PackOperation {
                key: key(3),
                kind: PackOpKind::Put(b"old".to_vec()),
            },
            PackOperation {
                key: key(1),
                kind: PackOpKind::Put(Vec::new()),
            },
            PackOperation {
                key: key(2),
                kind: PackOpKind::Tombstone,
            },
            PackOperation {
                key: key(3),
                kind: PackOpKind::Put(b"new".to_vec()),
            },
        ];
        let (metadata, values, _) = encode_frame_payload(&operations)?;
        let context = PackFrameContext::new(100, 105, [0x11; 32], [0x22; 32]);
        let header = encode_frame_header(
            7,
            context,
            operations.len(),
            metadata.len(),
            values.len(),
            frame_metadata_digest(&metadata),
            frame_value_digest(&values),
        )?;
        let footer = encode_frame_footer(
            7,
            (FRAME_HEADER_LEN + metadata.len() + values.len() + FRAME_FOOTER_LEN) as u64,
            frame_digest(&header),
        )?;
        Ok((header, metadata, values, footer))
    }

    fn hex(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            encoded.push(DIGITS[(byte >> 4) as usize] as char);
            encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
        }
        encoded
    }

    #[test]
    fn frame_v2_golden_bytes_and_row_semantics_are_stable() -> Result<()> {
        let (header, metadata, values, footer) = golden_parts()?;
        let frame = [
            header.as_slice(),
            metadata.as_slice(),
            values.as_slice(),
            footer.as_slice(),
        ]
        .concat();
        assert_eq!(hex(&frame), GOLDEN_FRAME_V2_HEX);
        let (_, stats) = validate_complete_frame(&frame, 7, 64, 64 + frame.len() as u64)?;
        assert_eq!(stats.rows, 4);
        assert_eq!(stats.puts, 3);
        assert_eq!(stats.tombstones, 1);
        assert_eq!(stats.value_bytes, 6);
        let entries = decode_frame_metadata(64, &metadata, values.len() as u64)?;
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.sequence)
                .collect::<Vec<_>>(),
            vec![1, 2, 0, 3]
        );
        assert!(!entries[0].tombstone);
        assert_eq!(entries[0].value_len, 0);
        assert!(entries[1].tombstone);
        assert_eq!(entries[1].value_offset, 0);
        Ok(())
    }

    #[test]
    fn unknown_numeric_frame_magic_fails_closed() -> Result<()> {
        let (mut header, _, _, _) = golden_parts()?;
        header[..8].copy_from_slice(b"N3PACK03");
        let error = validate_frame_header(&header, 7).expect_err("new frame version must fail");
        let typed = error
            .downcast_ref::<PackStoreError>()
            .expect("typed version error");
        assert!(matches!(
            typed,
            PackStoreError::UnsupportedVersion {
                artifact: PackStoreArtifact::Frame,
                found: 3,
                ..
            }
        ));

        let (mut header, _, _, _) = golden_parts()?;
        header[8..12].copy_from_slice(&1u32.to_le_bytes());
        let error = validate_frame_header(&header, 7).expect_err("old frame version must fail");
        assert!(matches!(
            error.downcast_ref::<PackStoreError>(),
            Some(PackStoreError::UnsupportedVersion { found: 1, .. })
        ));
        Ok(())
    }

    #[test]
    fn frame_v2_authenticates_every_section() -> Result<()> {
        let (header, metadata, values, footer) = golden_parts()?;
        let frame = [
            header.as_slice(),
            metadata.as_slice(),
            values.as_slice(),
            footer.as_slice(),
        ]
        .concat();
        for offset in [
            0,
            FRAME_HEADER_LEN,
            FRAME_HEADER_LEN + metadata.len(),
            frame.len() - 1,
        ] {
            let mut damaged = frame.clone();
            damaged[offset] ^= 0x80;
            assert!(validate_complete_frame(&damaged, 7, 64, 64 + damaged.len() as u64).is_err());
        }
        Ok(())
    }

    #[test]
    fn frame_v2_rejects_truncation_at_every_section_boundary() -> Result<()> {
        let (header, metadata, values, footer) = golden_parts()?;
        let frame = [
            header.as_slice(),
            metadata.as_slice(),
            values.as_slice(),
            footer.as_slice(),
        ]
        .concat();
        let metadata_end = FRAME_HEADER_LEN + metadata.len();
        let value_end = metadata_end + values.len();
        for cut in [
            1,
            FRAME_HEADER_LEN - 1,
            FRAME_HEADER_LEN,
            metadata_end - 1,
            metadata_end,
            value_end - 1,
            value_end,
            frame.len() - 1,
        ] {
            assert!(
                validate_complete_frame(&frame[..cut], 7, 64, 64 + cut as u64).is_err(),
                "truncation at byte {cut} must fail"
            );
        }
        Ok(())
    }

    #[test]
    fn frame_v2_rejects_noncanonical_metadata() -> Result<()> {
        let (_, metadata, values, _) = golden_parts()?;
        for offset in [33usize, 52] {
            let mut damaged = metadata.clone();
            damaged[offset] = 1;
            assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        }
        let mut damaged = metadata.clone();
        damaged[32] = 2;
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        let mut damaged = metadata.clone();
        damaged[36..40].copy_from_slice(&2u32.to_le_bytes());
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        let mut damaged = metadata.clone();
        damaged[36..40].copy_from_slice(&4u32.to_le_bytes());
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        let tombstone = FRAME_ROW_METADATA_LEN;
        let mut damaged = metadata.clone();
        damaged[tombstone + 40..tombstone + 48].copy_from_slice(&1u64.to_le_bytes());
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        let mut damaged = metadata.clone();
        damaged[tombstone + 48..tombstone + 52].copy_from_slice(&1u32.to_le_bytes());
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        let last = 3 * FRAME_ROW_METADATA_LEN;
        let mut damaged = metadata.clone();
        damaged[last + 40..last + 48].copy_from_slice(&4u64.to_le_bytes());
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        let mut damaged = metadata.clone();
        damaged[..32].fill(0xff);
        assert!(validate_payload_rows(&damaged, &values, 4).is_err());
        Ok(())
    }

    #[test]
    fn streaming_progress_covers_each_section_once_with_independent_cursors() -> Result<()> {
        let operations = (0u32..9_000)
            .map(|ordinal| {
                let mut key = [0u8; PACK_KEY_BYTES];
                key[0] = FRAME_NODE_KEY_PREFIX;
                key[PACK_KEY_BYTES - 4..].copy_from_slice(&ordinal.to_be_bytes());
                PackOperation {
                    key,
                    kind: PackOpKind::Put(vec![ordinal as u8; 128]),
                }
            })
            .collect::<Vec<_>>();
        let (metadata, values, _) = encode_frame_payload(&operations)?;
        let mut events = Vec::new();
        let stats = validate_payload_rows_with_progress(
            &metadata,
            &values,
            operations.len(),
            &mut |_, _, _| Ok(()),
            &mut |section, chunk, consumed| {
                events.push((section, chunk.to_vec(), consumed));
                Ok(())
            },
        )?;
        assert_eq!(stats.rows, operations.len() as u64);
        assert_eq!(
            events.first().map(|event| event.0),
            Some(FramePayloadSection::Values),
            "the faster value cursor must report before metadata's final remainder"
        );

        for (section, expected) in [
            (FramePayloadSection::Metadata, metadata.as_slice()),
            (FramePayloadSection::Values, values.as_slice()),
        ] {
            let mut previous = 0usize;
            let mut reconstructed = Vec::new();
            for (_, chunk, consumed) in events.iter().filter(|event| event.0 == section) {
                assert!(*consumed > previous, "section cursor must be monotone");
                assert_eq!(chunk.len(), *consumed - previous);
                reconstructed.extend_from_slice(chunk);
                previous = *consumed;
            }
            assert_eq!(previous, expected.len());
            assert_eq!(reconstructed, expected);
        }
        Ok(())
    }
}
