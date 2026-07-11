//! Fixed-width index records shared by MDBX publication and archive scanning.

use xxhash_rust::xxh3::Xxh3;

use crate::format::{EncodedFrame, FILE_HEADER_LEN, FRAME_HEADER_LEN, FrameHeader, FrameIndexRow};
use crate::{StaticFileError, StaticFileResult};

const ROW_LOCATION_LEN: usize = 44;
const FRAME_LOCATION_LEN: usize = 24;
const INDEX_STATE_LEN: usize = 56;
const INDEX_STATE_MAGIC: &[u8; 8] = b"NRSIDX01";
const INDEX_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RowLocation {
    pub(crate) height: u32,
    pub(crate) payload_offset: u64,
    pub(crate) compressed_len: u32,
    pub(crate) uncompressed_len: u32,
    pub(crate) payload_checksum: u64,
    pub(crate) value_offset: u32,
    pub(crate) value_len: u32,
}

impl RowLocation {
    pub(crate) fn from_frame(frame_offset: u64, header: FrameHeader, row: &FrameIndexRow) -> Self {
        Self {
            height: header.height,
            payload_offset: frame_offset
                + u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64")
                + u64::from(header.index_len),
            compressed_len: header.compressed_len,
            uncompressed_len: header.uncompressed_len,
            payload_checksum: header.payload_checksum,
            value_offset: row.value_offset,
            value_len: row.value_len,
        }
    }

    pub(crate) fn encode(self, key: &[u8]) -> [u8; ROW_LOCATION_LEN] {
        let mut bytes = [0u8; ROW_LOCATION_LEN];
        bytes[..4].copy_from_slice(&self.height.to_be_bytes());
        bytes[4..12].copy_from_slice(&self.payload_offset.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.compressed_len.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.uncompressed_len.to_le_bytes());
        bytes[20..28].copy_from_slice(&self.payload_checksum.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.value_offset.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.value_len.to_le_bytes());
        let checksum = keyed_checksum(key, &bytes[..36]);
        bytes[36..44].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    pub(crate) fn decode(key: &[u8], bytes: &[u8]) -> StaticFileResult<Self> {
        if bytes.len() != ROW_LOCATION_LEN {
            return Err(StaticFileError::invalid_index(
                "row location has an unexpected length",
            ));
        }
        if read_u64(bytes, 36)? != keyed_checksum(key, &bytes[..36]) {
            return Err(StaticFileError::invalid_index(
                "row location checksum mismatch",
            ));
        }
        Ok(Self {
            height: u32::from_be_bytes(read_array(bytes, 0)?),
            payload_offset: read_u64(bytes, 4)?,
            compressed_len: read_u32(bytes, 12)?,
            uncompressed_len: read_u32(bytes, 16)?,
            payload_checksum: read_u64(bytes, 20)?,
            value_offset: read_u32(bytes, 28)?,
            value_len: read_u32(bytes, 32)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FrameLocation {
    pub(crate) height: u32,
    pub(crate) start: u64,
    pub(crate) end: u64,
}

impl FrameLocation {
    pub(crate) fn from_frame(offset: u64, header: FrameHeader) -> StaticFileResult<Self> {
        let end = offset
            .checked_add(header.frame_len)
            .ok_or_else(|| StaticFileError::invalid(offset, "frame end overflow"))?;
        Ok(Self {
            height: header.height,
            start: offset,
            end,
        })
    }

    pub(crate) fn encode(self) -> [u8; FRAME_LOCATION_LEN] {
        let mut bytes = [0u8; FRAME_LOCATION_LEN];
        bytes[..8].copy_from_slice(&self.start.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.end.to_le_bytes());
        let checksum = keyed_checksum(&self.height.to_be_bytes(), &bytes[..16]);
        bytes[16..24].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    pub(crate) fn decode(height: u32, bytes: &[u8]) -> StaticFileResult<Self> {
        if bytes.len() != FRAME_LOCATION_LEN {
            return Err(StaticFileError::invalid_index(
                "frame location has an unexpected length",
            ));
        }
        if read_u64(bytes, 16)? != keyed_checksum(&height.to_be_bytes(), &bytes[..16]) {
            return Err(StaticFileError::invalid_index(
                "frame location checksum mismatch",
            ));
        }
        let start = read_u64(bytes, 0)?;
        let end = read_u64(bytes, 8)?;
        if end <= start {
            return Err(StaticFileError::invalid_index(
                "frame location end must follow its start",
            ));
        }
        Ok(Self { height, start, end })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IndexState {
    pub(crate) archive_id: u64,
    pub(crate) indexed_file_len: u64,
    pub(crate) tip: Option<u32>,
    pub(crate) last_frame_start: u64,
    pub(crate) row_versions: u64,
    pub(crate) tail_recovery_safe: bool,
}

impl IndexState {
    pub(crate) fn empty(archive_id: u64, tail_recovery_safe: bool) -> Self {
        let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
        Self {
            archive_id,
            indexed_file_len: header_len,
            tip: None,
            last_frame_start: header_len,
            row_versions: 0,
            tail_recovery_safe,
        }
    }

    pub(crate) fn next_height(self) -> StaticFileResult<u32> {
        self.tip.map_or(Ok(0), |height| {
            height.checked_add(1).ok_or_else(|| {
                StaticFileError::invalid_index("index cannot advance past maximum block height")
            })
        })
    }

    pub(crate) fn advance<F: IndexedFrame>(&mut self, frame: &F) -> StaticFileResult<()> {
        let header = frame.header();
        if frame.offset() != self.indexed_file_len {
            return Err(StaticFileError::invalid_index(
                "published frame does not begin at the indexed file length",
            ));
        }
        let expected = self.next_height()?;
        if header.height != expected {
            return Err(StaticFileError::NonContiguous {
                expected,
                actual: header.height,
            });
        }
        self.tip = Some(header.height);
        self.last_frame_start = frame.offset();
        self.indexed_file_len = frame
            .offset()
            .checked_add(header.frame_len)
            .ok_or_else(|| StaticFileError::invalid_index("indexed file length overflow"))?;
        self.row_versions = self
            .row_versions
            .checked_add(u64::try_from(frame.rows().len()).unwrap_or(u64::MAX))
            .ok_or_else(|| StaticFileError::invalid_index("row-version count overflow"))?;
        Ok(())
    }

    pub(crate) fn encode(self) -> [u8; INDEX_STATE_LEN] {
        let mut bytes = [0u8; INDEX_STATE_LEN];
        bytes[..8].copy_from_slice(INDEX_STATE_MAGIC);
        bytes[8..10].copy_from_slice(&INDEX_SCHEMA_VERSION.to_le_bytes());
        bytes[10] = u8::from(self.tip.is_some());
        bytes[11] = u8::from(self.tail_recovery_safe);
        bytes[12..16].copy_from_slice(&self.tip.unwrap_or_default().to_le_bytes());
        bytes[16..24].copy_from_slice(&self.archive_id.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.indexed_file_len.to_le_bytes());
        bytes[32..40].copy_from_slice(&self.last_frame_start.to_le_bytes());
        bytes[40..48].copy_from_slice(&self.row_versions.to_le_bytes());
        let checksum = keyed_checksum(INDEX_STATE_MAGIC, &bytes[..48]);
        bytes[48..56].copy_from_slice(&checksum.to_le_bytes());
        bytes
    }

    pub(crate) fn decode(bytes: &[u8]) -> StaticFileResult<Self> {
        if bytes.len() != INDEX_STATE_LEN || bytes.get(..8) != Some(INDEX_STATE_MAGIC.as_slice()) {
            return Err(StaticFileError::invalid_index(
                "index state magic or length mismatch",
            ));
        }
        let version = u16::from_le_bytes(read_array(bytes, 8)?);
        if version != INDEX_SCHEMA_VERSION {
            return Err(StaticFileError::invalid_index(format!(
                "unsupported index schema version {version}"
            )));
        }
        if read_u64(bytes, 48)? != keyed_checksum(INDEX_STATE_MAGIC, &bytes[..48]) {
            return Err(StaticFileError::invalid_index(
                "index state checksum mismatch",
            ));
        }
        let tip = match bytes[10] {
            0 => None,
            1 => Some(read_u32(bytes, 12)?),
            _ => {
                return Err(StaticFileError::invalid_index(
                    "index state has an invalid tip marker",
                ));
            }
        };
        let tail_recovery_safe = match bytes[11] {
            0 => false,
            1 => true,
            _ => {
                return Err(StaticFileError::invalid_index(
                    "index state has an invalid recovery marker",
                ));
            }
        };
        let state = Self {
            archive_id: read_u64(bytes, 16)?,
            indexed_file_len: read_u64(bytes, 24)?,
            tip,
            last_frame_start: read_u64(bytes, 32)?,
            row_versions: read_u64(bytes, 40)?,
            tail_recovery_safe,
        };
        let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
        if state.archive_id == 0
            || state.indexed_file_len < header_len
            || (state.tip.is_none()
                && (state.indexed_file_len != header_len
                    || state.last_frame_start != header_len
                    || state.row_versions != 0))
            || (state.tip.is_some()
                && (state.last_frame_start < header_len
                    || state.indexed_file_len <= state.last_frame_start
                    || state.row_versions == 0))
        {
            return Err(StaticFileError::invalid_index(
                "index state invariants are inconsistent",
            ));
        }
        Ok(state)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ScannedFrame {
    pub(crate) offset: u64,
    pub(crate) header: FrameHeader,
    pub(crate) rows: Vec<FrameIndexRow>,
}

pub(crate) trait IndexedFrame {
    fn offset(&self) -> u64;
    fn header(&self) -> FrameHeader;
    fn rows(&self) -> &[FrameIndexRow];
}

impl IndexedFrame for ScannedFrame {
    fn offset(&self) -> u64 {
        self.offset
    }

    fn header(&self) -> FrameHeader {
        self.header
    }

    fn rows(&self) -> &[FrameIndexRow] {
        &self.rows
    }
}

pub(crate) struct PositionedEncodedFrame {
    pub(crate) offset: u64,
    header: FrameHeader,
    rows: Vec<FrameIndexRow>,
}

impl PositionedEncodedFrame {
    pub(crate) fn new(offset: u64, frame: EncodedFrame) -> Self {
        Self {
            offset,
            header: frame.header,
            rows: frame.rows,
        }
    }
}

impl IndexedFrame for PositionedEncodedFrame {
    fn offset(&self) -> u64 {
        self.offset
    }

    fn header(&self) -> FrameHeader {
        self.header
    }

    fn rows(&self) -> &[FrameIndexRow] {
        &self.rows
    }
}

fn keyed_checksum(key: &[u8], data: &[u8]) -> u64 {
    let mut hasher = Xxh3::new();
    hasher.update(key);
    hasher.update(data);
    hasher.digest()
}

fn read_u32(bytes: &[u8], offset: usize) -> StaticFileResult<u32> {
    Ok(u32::from_le_bytes(read_array(bytes, offset)?))
}

fn read_u64(bytes: &[u8], offset: usize) -> StaticFileResult<u64> {
    Ok(u64::from_le_bytes(read_array(bytes, offset)?))
}

fn read_array<const N: usize>(bytes: &[u8], offset: usize) -> StaticFileResult<[u8; N]> {
    let end = offset
        .checked_add(N)
        .ok_or_else(|| StaticFileError::invalid_index("index decoder offset overflow"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| StaticFileError::invalid_index("truncated index value"))?;
    let mut out = [0u8; N];
    out.copy_from_slice(value);
    Ok(out)
}
