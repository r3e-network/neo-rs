use super::*;

pub(super) fn encode_frame_payload(
    frame_start: u64,
    operations: &[PackOperation],
) -> Result<(Vec<u8>, Vec<IndexEntry>)> {
    ensure!(
        !operations.is_empty(),
        "frame must contain at least one row"
    );
    let operation_count =
        u64::try_from(operations.len()).context("frame row count overflows u64")?;
    ensure!(
        operation_count <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let estimated = operations.iter().try_fold(0usize, |total, operation| {
        let value_len = match &operation.kind {
            PackOpKind::Put(value) => value.len(),
            PackOpKind::Tombstone => 0,
        };
        total
            .checked_add(PACK_KEY_BYTES + 1 + 4)
            .and_then(|total| total.checked_add(value_len))
            .context("frame payload size overflows usize")
    })?;
    let estimated_u64 = u64::try_from(estimated).context("frame payload size overflows u64")?;
    ensure!(
        estimated_u64 <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    let mut payload = Vec::with_capacity(estimated);
    let mut entries = Vec::with_capacity(operations.len());
    for (sequence, operation) in operations.iter().enumerate() {
        payload.extend_from_slice(&operation.key);
        let value_start = payload
            .len()
            .checked_add(1 + 4)
            .context("frame value offset overflows usize")?;
        let (tombstone, value) = match &operation.kind {
            PackOpKind::Put(value) => (false, value.as_slice()),
            PackOpKind::Tombstone => (true, &[][..]),
        };
        payload.push(u8::from(!tombstone));
        let value_len = u32::try_from(value.len()).context("frame value exceeds u32")?;
        payload.extend_from_slice(&value_len.to_le_bytes());
        payload.extend_from_slice(value);
        let value_offset = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|offset| offset.checked_add(value_start as u64))
            .context("absolute frame value offset overflows u64")?;
        entries.push(IndexEntry {
            key: operation.key,
            sequence: u32::try_from(sequence).context("frame sequence exceeds u32")?,
            value_offset,
            value_len,
            tombstone,
        });
    }
    ensure!(
        payload.len() == estimated,
        "encoded frame length changed unexpectedly"
    );
    Ok((payload, entries))
}

/// Reconstructs one frame's index entries from its payload rows. Used only
/// by the rebuild path; offsets point at the original payload bytes.
pub(super) fn decode_frame_payload(frame_start: u64, payload: &[u8]) -> Result<Vec<IndexEntry>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    let mut sequence = 0u32;
    while offset < payload.len() {
        let header_end = offset
            .checked_add(PACK_KEY_BYTES + 1 + 4)
            .context("row header offset overflows")?;
        ensure!(header_end <= payload.len(), "truncated frame row header");
        let mut key = [0u8; PACK_KEY_BYTES];
        key.copy_from_slice(&payload[offset..offset + PACK_KEY_BYTES]);
        let kind = payload[offset + PACK_KEY_BYTES];
        ensure!(kind <= 1, "invalid frame row kind");
        let value_len = u32_at(payload, offset + PACK_KEY_BYTES + 1)?;
        ensure!(kind == 1 || value_len == 0, "tombstone carries a value");
        let value_start = header_end;
        let value_end = value_start
            .checked_add(value_len as usize)
            .context("row value offset overflows")?;
        ensure!(value_end <= payload.len(), "truncated frame row value");
        let value_offset = frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|offset| offset.checked_add(value_start as u64))
            .context("absolute frame value offset overflows u64")?;
        entries.push(IndexEntry {
            key,
            sequence,
            value_offset,
            value_len,
            tombstone: kind == 0,
        });
        sequence = sequence
            .checked_add(1)
            .context("frame sequence exceeds u32")?;
        offset = value_end;
    }
    Ok(entries)
}

pub(super) fn encode_frame_header(
    epoch: u64,
    rows: usize,
    payload_len: usize,
    checksum: [u8; 32],
) -> Result<[u8; FRAME_HEADER_LEN]> {
    ensure!(rows > 0, "frame must contain at least one row");
    let rows_u64 = u64::try_from(rows).context("frame row count does not fit u64")?;
    ensure!(
        rows_u64 <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let payload_len_u64 =
        u64::try_from(payload_len).context("frame payload length does not fit u64")?;
    ensure!(
        payload_len_u64 <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    let minimum_payload = rows_u64
        .checked_mul(FRAME_ROW_HEADER_BYTES)
        .context("minimum frame payload length overflows")?;
    ensure!(
        payload_len_u64 >= minimum_payload,
        "frame payload is too short for its declared row count"
    );
    let mut bytes = [0u8; FRAME_HEADER_LEN];
    bytes[0..8].copy_from_slice(FRAME_MAGIC);
    bytes[8..12].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
    bytes[12..16].copy_from_slice(&(FRAME_HEADER_LEN as u32).to_le_bytes());
    bytes[16..24].copy_from_slice(&epoch.to_le_bytes());
    bytes[24..32].copy_from_slice(&rows_u64.to_le_bytes());
    bytes[32..40].copy_from_slice(&payload_len_u64.to_le_bytes());
    bytes[40..72].copy_from_slice(&checksum);
    Ok(bytes)
}

/// Validates a frame header and returns its payload length.
pub(super) fn validate_frame_header(
    header: &[u8; FRAME_HEADER_LEN],
    expected_epoch: u64,
) -> Result<u64> {
    ensure!(&header[0..8] == FRAME_MAGIC, "invalid frame magic");
    ensure!(
        u32_at(header, 8)? == PACK_FRAME_FORMAT_VERSION,
        "unsupported frame version"
    );
    ensure!(
        u32_at(header, 12)? as usize == FRAME_HEADER_LEN,
        "invalid frame header length"
    );
    ensure!(
        u64_at(header, 16)? == expected_epoch,
        "non-contiguous frame epoch"
    );
    let rows = u64_at(header, 24)?;
    ensure!(rows > 0, "frame must contain at least one row");
    ensure!(
        rows <= MAX_FRAME_ROWS,
        "frame row count exceeds the hard limit of {MAX_FRAME_ROWS}"
    );
    let payload_len = u64_at(header, 32)?;
    ensure!(
        payload_len <= MAX_FRAME_PAYLOAD_BYTES,
        "frame payload exceeds the hard limit of {MAX_FRAME_PAYLOAD_BYTES} bytes"
    );
    let minimum_payload = rows
        .checked_mul(FRAME_ROW_HEADER_BYTES)
        .context("minimum frame payload length overflows")?;
    ensure!(
        payload_len >= minimum_payload,
        "frame payload is too short for its declared row count"
    );
    Ok(payload_len)
}

/// Validated frame end offsets. Anything beyond the last complete,
/// well-formed frame is a torn or orphaned tail handled by the caller.
pub(super) struct FrameScan {
    pub(super) frame_ends: Vec<u64>,
}

/// Walks frame headers without reading payloads; stops at the first torn or
/// malformed frame start so the caller can truncate the uncommitted tail.
pub(super) fn scan_frames(pack: &File) -> Result<FrameScan> {
    let file_len = pack.metadata().context("stat append pack")?.len();
    let mut frame_ends = Vec::new();
    let mut offset = 0u64;
    let mut expected_epoch = 0u64;
    while offset < file_len {
        let mut header = [0u8; FRAME_HEADER_LEN];
        let mut filled = 0usize;
        while filled < FRAME_HEADER_LEN {
            let read = pack
                .read_at(&mut header[filled..], offset + filled as u64)
                .context("read frame header")?;
            if read == 0 {
                break;
            }
            filled += read;
        }
        if filled < FRAME_HEADER_LEN {
            break;
        }
        let Ok(payload_len) = validate_frame_header(&header, expected_epoch) else {
            break;
        };
        let frame_end = offset
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload_len))
            .context("frame end offset overflows")?;
        if frame_end > file_len {
            break;
        }
        frame_ends.push(frame_end);
        offset = frame_end;
        expected_epoch = expected_epoch
            .checked_add(1)
            .context("frame epoch overflows")?;
    }
    Ok(FrameScan { frame_ends })
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
        0
    } else {
        scan.frame_ends[index - 1]
    };
    read_frame_receipt_at(pack, epoch, frame_start, frame_end)
}

pub(super) fn read_frame_receipt_at(
    pack: &File,
    epoch: u64,
    frame_start: u64,
    frame_end: u64,
) -> Result<PackFrameReceipt> {
    let mut header = [0u8; FRAME_HEADER_LEN];
    pack.read_exact_at(&mut header, frame_start)
        .with_context(|| format!("read frame {epoch} header"))?;
    let payload_bytes = validate_frame_header(&header, epoch)?;
    ensure!(
        frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload_bytes))
            == Some(frame_end),
        "frame {epoch} length does not match the validated frame chain"
    );
    Ok(PackFrameReceipt {
        epoch,
        frame_start,
        frame_end,
        rows: u64_at(&header, 24)?,
        payload_bytes,
        payload_sha256: header[40..72].try_into().expect("frame payload checksum"),
    })
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
        0
    } else {
        *scan
            .frame_ends
            .get(expected - 1)
            .context("committed frame prefix is incomplete")?
    };
    let pack_path = root.join("frames.pack");
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

/// Fully verifies the committed tail frame: header, payload checksum, and
/// row structure. This is the only payload read during open.
pub(super) fn verify_tail_frame(
    pack: &Mmap,
    frame_start: u64,
    frame_end: u64,
    epoch: u64,
) -> Result<()> {
    let start = usize::try_from(frame_start).context("tail frame offset does not fit usize")?;
    let header: &[u8; FRAME_HEADER_LEN] = pack
        .as_slice()
        .get(start..start + FRAME_HEADER_LEN)
        .context("read committed tail frame header")?
        .try_into()
        .expect("frame header length");
    let payload_len = validate_frame_header(header, epoch)?;
    let row_count = usize::try_from(u64_at(header, 24)?).context("row count does not fit usize")?;
    ensure!(
        frame_start
            .checked_add(FRAME_HEADER_LEN as u64)
            .and_then(|end| end.checked_add(payload_len))
            == Some(frame_end),
        "committed tail frame length mismatch"
    );
    let payload_end = usize::try_from(frame_end).context("tail frame end does not fit usize")?;
    let payload = pack
        .as_slice()
        .get(start + FRAME_HEADER_LEN..payload_end)
        .context("read committed tail frame payload")?;
    ensure!(
        digest(payload).as_slice() == &header[40..72],
        "frame payload checksum mismatch in committed tail frame"
    );
    validate_payload_rows(payload, row_count)?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PayloadRowStats {
    pub(super) rows: u64,
    pub(super) puts: u64,
    pub(super) tombstones: u64,
    pub(super) value_bytes: u64,
}

pub(super) fn validate_payload_rows(
    payload: &[u8],
    expected_rows: usize,
) -> Result<PayloadRowStats> {
    validate_payload_rows_with(payload, expected_rows, &mut |_, _, _| Ok(()))
}

pub(super) fn validate_payload_rows_with<F>(
    payload: &[u8],
    expected_rows: usize,
    visit: &mut F,
) -> Result<PayloadRowStats>
where
    F: FnMut(&[u8; PACK_KEY_BYTES], u8, &[u8]) -> Result<()>,
{
    let mut offset = 0usize;
    let mut rows = 0usize;
    let mut stats = PayloadRowStats::default();
    while offset < payload.len() {
        let header_end = offset
            .checked_add(PACK_KEY_BYTES + 1 + 4)
            .context("row header offset overflows")?;
        ensure!(header_end <= payload.len(), "truncated frame row header");
        let key: &[u8; PACK_KEY_BYTES] = payload[offset..offset + PACK_KEY_BYTES]
            .try_into()
            .expect("validated row key length");
        let kind = payload[offset + PACK_KEY_BYTES];
        ensure!(kind <= 1, "invalid frame row kind");
        let value_len = u32_at(payload, offset + PACK_KEY_BYTES + 1)? as usize;
        ensure!(kind == 1 || value_len == 0, "tombstone carries a value");
        let value_end = header_end
            .checked_add(value_len)
            .context("row value offset overflows")?;
        let value = payload
            .get(header_end..value_end)
            .context("truncated frame row value")?;
        visit(key, kind, value)?;
        if kind == 0 {
            stats.tombstones = stats.tombstones.saturating_add(1);
        } else {
            stats.puts = stats.puts.saturating_add(1);
            stats.value_bytes = stats.value_bytes.saturating_add(
                u64::try_from(value_len).context("row value length does not fit u64")?,
            );
        }
        offset = value_end;
        rows = rows.checked_add(1).context("frame row count overflows")?;
    }
    ensure!(rows == expected_rows, "frame row count mismatch");
    stats.rows = u64::try_from(rows).context("frame row count does not fit u64")?;
    Ok(stats)
}
