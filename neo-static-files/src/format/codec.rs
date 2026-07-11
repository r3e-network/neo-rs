//! Versioned static-file frame encoding and validation.

use std::collections::BTreeSet;

use xxhash_rust::xxh3::xxh3_64;

use crate::{StaticFileConfig, StaticFileError, StaticFileResult, StaticRecord};

pub(crate) const FILE_HEADER_LEN: usize = 32;
pub(crate) const FRAME_HEADER_LEN: usize = 56;
pub(crate) const FRAME_FOOTER_LEN: usize = 16;
#[cfg(test)]
pub(crate) const ROW_INDEX_FIXED_LEN: usize = 10;

const FILE_MAGIC: &[u8; 8] = b"NEORSF01";
const FRAME_MAGIC: &[u8; 8] = b"NRSFRM01";
const FRAME_FOOTER_MAGIC: &[u8; 8] = b"NRSEND01";
const FORMAT_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameHeader {
    pub(crate) frame_len: u64,
    pub(crate) height: u32,
    pub(crate) row_count: u32,
    pub(crate) index_len: u32,
    pub(crate) compressed_len: u32,
    pub(crate) uncompressed_len: u32,
    pub(crate) index_checksum: u64,
    pub(crate) payload_checksum: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct EncodedRow {
    pub(crate) key: Box<[u8]>,
    pub(crate) value_offset: u32,
    pub(crate) value_len: u32,
}

#[derive(Clone, Debug)]
pub(crate) struct EncodedFrame {
    pub(crate) bytes: Vec<u8>,
    pub(crate) header: FrameHeader,
    pub(crate) rows: Vec<EncodedRow>,
}

#[derive(Clone, Debug)]
pub(crate) struct ParsedIndexRow {
    pub(crate) key: Box<[u8]>,
    pub(crate) value_offset: u32,
    pub(crate) value_len: u32,
}

pub(crate) fn file_header() -> [u8; FILE_HEADER_LEN] {
    let mut header = [0u8; FILE_HEADER_LEN];
    header[..8].copy_from_slice(FILE_MAGIC);
    header[8..10].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    header[10..12].copy_from_slice(&(FILE_HEADER_LEN as u16).to_le_bytes());
    let checksum = xxh3_64(&header[..16]);
    header[16..24].copy_from_slice(&checksum.to_le_bytes());
    header
}

pub(crate) fn validate_file_header(bytes: &[u8], offset: u64) -> StaticFileResult<()> {
    if bytes.len() != FILE_HEADER_LEN {
        return Err(StaticFileError::invalid(offset, "truncated file header"));
    }
    if &bytes[..8] != FILE_MAGIC {
        return Err(StaticFileError::invalid(offset, "file magic mismatch"));
    }
    let version = read_u16(bytes, 8, offset)?;
    if version != FORMAT_VERSION {
        return Err(StaticFileError::UnsupportedVersion {
            expected: FORMAT_VERSION,
            actual: version,
        });
    }
    let header_len = usize::from(read_u16(bytes, 10, offset)?);
    if header_len != FILE_HEADER_LEN {
        return Err(StaticFileError::invalid(
            offset,
            format!("unexpected file header length {header_len}"),
        ));
    }
    let stored_checksum = read_u64(bytes, 16, offset)?;
    if stored_checksum != xxh3_64(&bytes[..16]) {
        return Err(StaticFileError::invalid(
            offset,
            "file header checksum mismatch",
        ));
    }
    Ok(())
}

pub(crate) fn encode_frame(
    record: StaticRecord,
    config: StaticFileConfig,
) -> StaticFileResult<EncodedFrame> {
    let (height, rows) = record.into_parts();
    if rows.is_empty() {
        return Err(StaticFileError::EmptyRecord { height });
    }
    if rows.len() > config.max_rows_per_record {
        return Err(StaticFileError::LimitExceeded {
            height,
            limit: format!("max_rows_per_record={}", config.max_rows_per_record),
            actual: u64::try_from(rows.len()).unwrap_or(u64::MAX),
        });
    }

    let mut rows = rows
        .into_iter()
        .map(|row| row.into_parts())
        .collect::<Vec<_>>();
    rows.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    let mut seen = BTreeSet::new();
    for (key, _) in &rows {
        if !seen.insert(key.as_slice()) {
            return Err(StaticFileError::DuplicateKey {
                height,
                key_hash: xxh3_64(key),
            });
        }
        if key.len() > usize::from(u16::MAX) {
            return Err(StaticFileError::LimitExceeded {
                height,
                limit: format!("maximum row key length={}", u16::MAX),
                actual: u64::try_from(key.len()).unwrap_or(u64::MAX),
            });
        }
    }

    let mut index = Vec::new();
    let mut values = Vec::new();
    let mut encoded_rows = Vec::with_capacity(rows.len());
    for (key, value) in rows {
        let value_offset =
            u32::try_from(values.len()).map_err(|_| StaticFileError::LimitExceeded {
                height,
                limit: "maximum uncompressed value bytes=4294967295".to_string(),
                actual: u64::try_from(values.len()).unwrap_or(u64::MAX),
            })?;
        let value_len = u32::try_from(value.len()).map_err(|_| StaticFileError::LimitExceeded {
            height,
            limit: "maximum row value bytes=4294967295".to_string(),
            actual: u64::try_from(value.len()).unwrap_or(u64::MAX),
        })?;
        index.extend_from_slice(
            &u16::try_from(key.len())
                .expect("key length checked above")
                .to_le_bytes(),
        );
        index.extend_from_slice(&value_offset.to_le_bytes());
        index.extend_from_slice(&value_len.to_le_bytes());
        index.extend_from_slice(&key);
        values.extend_from_slice(&value);
        encoded_rows.push(EncodedRow {
            key: key.into_boxed_slice(),
            value_offset,
            value_len,
        });
    }

    if values.len() > config.max_uncompressed_record_bytes {
        return Err(StaticFileError::LimitExceeded {
            height,
            limit: format!(
                "max_uncompressed_record_bytes={}",
                config.max_uncompressed_record_bytes
            ),
            actual: u64::try_from(values.len()).unwrap_or(u64::MAX),
        });
    }
    let compressed = zstd::bulk::compress(&values, config.compression_level)
        .map_err(|error| StaticFileError::Compression(error.to_string()))?;
    let index_len = u32::try_from(index.len()).map_err(|_| StaticFileError::LimitExceeded {
        height,
        limit: "maximum encoded row index bytes=4294967295".to_string(),
        actual: u64::try_from(index.len()).unwrap_or(u64::MAX),
    })?;
    let compressed_len =
        u32::try_from(compressed.len()).map_err(|_| StaticFileError::LimitExceeded {
            height,
            limit: "maximum compressed payload bytes=4294967295".to_string(),
            actual: u64::try_from(compressed.len()).unwrap_or(u64::MAX),
        })?;
    let uncompressed_len =
        u32::try_from(values.len()).expect("configured record limit fits in u32");
    let frame_len_usize = FRAME_HEADER_LEN
        .checked_add(index.len())
        .and_then(|length| length.checked_add(compressed.len()))
        .and_then(|length| length.checked_add(FRAME_FOOTER_LEN))
        .ok_or_else(|| StaticFileError::LimitExceeded {
            height,
            limit: "addressable frame length".to_string(),
            actual: u64::MAX,
        })?;
    if frame_len_usize > config.max_frame_bytes {
        return Err(StaticFileError::LimitExceeded {
            height,
            limit: format!("max_frame_bytes={}", config.max_frame_bytes),
            actual: u64::try_from(frame_len_usize).unwrap_or(u64::MAX),
        });
    }
    let frame_len = u64::try_from(frame_len_usize).expect("usize fits u64 on supported targets");
    let header = FrameHeader {
        frame_len,
        height,
        row_count: u32::try_from(encoded_rows.len()).expect("row limit fits u32"),
        index_len,
        compressed_len,
        uncompressed_len,
        index_checksum: xxh3_64(&index),
        payload_checksum: xxh3_64(&values),
    };
    let header_bytes = encode_frame_header(header);
    let mut bytes = Vec::with_capacity(frame_len_usize);
    bytes.extend_from_slice(&header_bytes);
    bytes.extend_from_slice(&index);
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(FRAME_FOOTER_MAGIC);
    bytes.extend_from_slice(&xxh3_64(&header_bytes).to_le_bytes());
    debug_assert_eq!(bytes.len(), frame_len_usize);
    Ok(EncodedFrame {
        bytes,
        header,
        rows: encoded_rows,
    })
}

pub(crate) fn decode_frame_header(bytes: &[u8], offset: u64) -> StaticFileResult<FrameHeader> {
    if bytes.len() != FRAME_HEADER_LEN {
        return Err(StaticFileError::invalid(offset, "truncated frame header"));
    }
    if &bytes[..8] != FRAME_MAGIC {
        return Err(StaticFileError::invalid(offset, "frame magic mismatch"));
    }
    Ok(FrameHeader {
        frame_len: read_u64(bytes, 8, offset)?,
        height: read_u32(bytes, 16, offset)?,
        row_count: read_u32(bytes, 20, offset)?,
        index_len: read_u32(bytes, 24, offset)?,
        compressed_len: read_u32(bytes, 28, offset)?,
        uncompressed_len: read_u32(bytes, 32, offset)?,
        index_checksum: read_u64(bytes, 40, offset)?,
        payload_checksum: read_u64(bytes, 48, offset)?,
    })
}

pub(crate) fn validate_frame_shape(
    header: FrameHeader,
    offset: u64,
    config: StaticFileConfig,
) -> StaticFileResult<()> {
    let expected = u64::try_from(FRAME_HEADER_LEN + FRAME_FOOTER_LEN)
        .expect("fixed frame lengths fit u64")
        .checked_add(u64::from(header.index_len))
        .and_then(|length| length.checked_add(u64::from(header.compressed_len)))
        .ok_or_else(|| StaticFileError::invalid(offset, "frame length overflow"))?;
    if header.frame_len != expected {
        return Err(StaticFileError::invalid(
            offset,
            format!(
                "frame length {} does not match component length {expected}",
                header.frame_len
            ),
        ));
    }
    if header.frame_len
        > u64::try_from(config.max_frame_bytes).expect("usize fits u64 on supported targets")
    {
        return Err(StaticFileError::LimitExceeded {
            height: header.height,
            limit: format!("max_frame_bytes={}", config.max_frame_bytes),
            actual: header.frame_len,
        });
    }
    if usize::try_from(header.uncompressed_len).unwrap_or(usize::MAX)
        > config.max_uncompressed_record_bytes
    {
        return Err(StaticFileError::LimitExceeded {
            height: header.height,
            limit: format!(
                "max_uncompressed_record_bytes={}",
                config.max_uncompressed_record_bytes
            ),
            actual: u64::from(header.uncompressed_len),
        });
    }
    if usize::try_from(header.row_count).unwrap_or(usize::MAX) > config.max_rows_per_record {
        return Err(StaticFileError::LimitExceeded {
            height: header.height,
            limit: format!("max_rows_per_record={}", config.max_rows_per_record),
            actual: u64::from(header.row_count),
        });
    }
    Ok(())
}

pub(crate) fn parse_index(
    header: FrameHeader,
    index: &[u8],
    offset: u64,
) -> StaticFileResult<Vec<ParsedIndexRow>> {
    if xxh3_64(index) != header.index_checksum {
        return Err(StaticFileError::Checksum {
            height: header.height,
            component: "row-index",
        });
    }
    let mut cursor = 0usize;
    let mut rows = Vec::with_capacity(usize::try_from(header.row_count).unwrap_or(0));
    let mut seen = BTreeSet::new();
    for _ in 0..header.row_count {
        let key_len = usize::from(read_u16(index, cursor, offset)?);
        cursor = cursor
            .checked_add(2)
            .ok_or_else(|| StaticFileError::invalid(offset, "row-index cursor overflow"))?;
        let value_offset = read_u32(index, cursor, offset)?;
        cursor += 4;
        let value_len = read_u32(index, cursor, offset)?;
        cursor += 4;
        let key_end = cursor
            .checked_add(key_len)
            .ok_or_else(|| StaticFileError::invalid(offset, "row key length overflow"))?;
        let key = index.get(cursor..key_end).ok_or_else(|| {
            StaticFileError::invalid(offset, "row key extends beyond encoded index")
        })?;
        cursor = key_end;
        let value_end = value_offset
            .checked_add(value_len)
            .ok_or_else(|| StaticFileError::invalid(offset, "row value range overflow"))?;
        if value_end > header.uncompressed_len {
            return Err(StaticFileError::invalid(
                offset,
                "row value extends beyond uncompressed payload",
            ));
        }
        if !seen.insert(key) {
            return Err(StaticFileError::DuplicateKey {
                height: header.height,
                key_hash: xxh3_64(key),
            });
        }
        rows.push(ParsedIndexRow {
            key: key.to_vec().into_boxed_slice(),
            value_offset,
            value_len,
        });
    }
    if cursor != index.len() {
        return Err(StaticFileError::invalid(
            offset,
            format!("{} trailing row-index bytes", index.len() - cursor),
        ));
    }
    Ok(rows)
}

pub(crate) fn validate_footer(
    footer: &[u8],
    header_bytes: &[u8],
    offset: u64,
) -> StaticFileResult<()> {
    if footer.len() != FRAME_FOOTER_LEN {
        return Err(StaticFileError::invalid(offset, "truncated frame footer"));
    }
    if &footer[..8] != FRAME_FOOTER_MAGIC {
        return Err(StaticFileError::invalid(
            offset,
            "frame footer magic mismatch",
        ));
    }
    let checksum = read_u64(footer, 8, offset)?;
    if checksum != xxh3_64(header_bytes) {
        return Err(StaticFileError::invalid(
            offset,
            "frame header checksum mismatch",
        ));
    }
    Ok(())
}

pub(crate) fn decode_payload(
    height: u32,
    uncompressed_len: u32,
    payload_checksum: u64,
    compressed: &[u8],
) -> StaticFileResult<Vec<u8>> {
    let expected_len = usize::try_from(uncompressed_len).map_err(|_| {
        StaticFileError::invalid(0, "uncompressed payload length does not fit usize")
    })?;
    let payload = zstd::bulk::decompress(compressed, expected_len)
        .map_err(|error| StaticFileError::Compression(error.to_string()))?;
    if payload.len() != expected_len || xxh3_64(&payload) != payload_checksum {
        return Err(StaticFileError::Checksum {
            height,
            component: "payload",
        });
    }
    Ok(payload)
}

fn encode_frame_header(header: FrameHeader) -> [u8; FRAME_HEADER_LEN] {
    let mut bytes = [0u8; FRAME_HEADER_LEN];
    bytes[..8].copy_from_slice(FRAME_MAGIC);
    bytes[8..16].copy_from_slice(&header.frame_len.to_le_bytes());
    bytes[16..20].copy_from_slice(&header.height.to_le_bytes());
    bytes[20..24].copy_from_slice(&header.row_count.to_le_bytes());
    bytes[24..28].copy_from_slice(&header.index_len.to_le_bytes());
    bytes[28..32].copy_from_slice(&header.compressed_len.to_le_bytes());
    bytes[32..36].copy_from_slice(&header.uncompressed_len.to_le_bytes());
    bytes[40..48].copy_from_slice(&header.index_checksum.to_le_bytes());
    bytes[48..56].copy_from_slice(&header.payload_checksum.to_le_bytes());
    bytes
}

fn read_u16(bytes: &[u8], cursor: usize, offset: u64) -> StaticFileResult<u16> {
    let data = read_array::<2>(bytes, cursor, offset)?;
    Ok(u16::from_le_bytes(data))
}

fn read_u32(bytes: &[u8], cursor: usize, offset: u64) -> StaticFileResult<u32> {
    let data = read_array::<4>(bytes, cursor, offset)?;
    Ok(u32::from_le_bytes(data))
}

fn read_u64(bytes: &[u8], cursor: usize, offset: u64) -> StaticFileResult<u64> {
    let data = read_array::<8>(bytes, cursor, offset)?;
    Ok(u64::from_le_bytes(data))
}

fn read_array<const N: usize>(
    bytes: &[u8],
    cursor: usize,
    offset: u64,
) -> StaticFileResult<[u8; N]> {
    let end = cursor
        .checked_add(N)
        .ok_or_else(|| StaticFileError::invalid(offset, "decoder cursor overflow"))?;
    let slice = bytes
        .get(cursor..end)
        .ok_or_else(|| StaticFileError::invalid(offset, "truncated numeric field"))?;
    let mut out = [0u8; N];
    out.copy_from_slice(slice);
    Ok(out)
}
