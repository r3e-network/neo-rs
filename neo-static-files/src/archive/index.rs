//! In-memory latest-row index and complete-frame scanner.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use hashbrown::HashMap;

use super::StaticFileConfig;
use super::io::read_exact_at;
use crate::format::{
    EncodedFrame, FILE_HEADER_LEN, FRAME_FOOTER_LEN, FRAME_HEADER_LEN, FrameHeader, ParsedIndexRow,
    decode_frame_header, decode_payload, parse_index, validate_footer, validate_frame_shape,
};
use crate::{StaticFileError, StaticFileResult};

#[derive(Clone, Copy, Debug)]
pub(super) struct RowLocation {
    pub(super) height: u32,
    pub(super) payload_offset: u64,
    pub(super) compressed_len: u32,
    pub(super) uncompressed_len: u32,
    pub(super) payload_checksum: u64,
    pub(super) value_offset: u32,
    pub(super) value_len: u32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FrameLocation {
    pub(super) end: u64,
}

#[derive(Debug)]
pub(super) struct ArchiveIndex {
    pub(super) rows: HashMap<Box<[u8]>, RowLocation>,
    pub(super) frames: BTreeMap<u32, FrameLocation>,
    pub(super) file_len: u64,
}

impl ArchiveIndex {
    pub(super) fn empty() -> Self {
        Self {
            rows: HashMap::new(),
            frames: BTreeMap::new(),
            file_len: u64::try_from(FILE_HEADER_LEN).expect("header length fits u64"),
        }
    }

    pub(super) fn tip(&self) -> Option<u32> {
        self.frames.last_key_value().map(|(height, _)| *height)
    }

    pub(super) fn insert_encoded_frame(&mut self, offset: u64, frame: &EncodedFrame) {
        let payload_offset = offset
            + u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64")
            + u64::from(frame.header.index_len);
        for row in &frame.rows {
            self.rows.insert(
                row.key.clone(),
                RowLocation {
                    height: frame.header.height,
                    payload_offset,
                    compressed_len: frame.header.compressed_len,
                    uncompressed_len: frame.header.uncompressed_len,
                    payload_checksum: frame.header.payload_checksum,
                    value_offset: row.value_offset,
                    value_len: row.value_len,
                },
            );
        }
        self.frames.insert(
            frame.header.height,
            FrameLocation {
                end: offset + frame.header.frame_len,
            },
        );
    }

    fn insert_scanned_frame(
        &mut self,
        offset: u64,
        header: FrameHeader,
        rows: Vec<ParsedIndexRow>,
    ) {
        let payload_offset = offset
            + u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64")
            + u64::from(header.index_len);
        for row in rows {
            self.rows.insert(
                row.key,
                RowLocation {
                    height: header.height,
                    payload_offset,
                    compressed_len: header.compressed_len,
                    uncompressed_len: header.uncompressed_len,
                    payload_checksum: header.payload_checksum,
                    value_offset: row.value_offset,
                    value_len: row.value_len,
                },
            );
        }
        self.frames.insert(
            header.height,
            FrameLocation {
                end: offset + header.frame_len,
            },
        );
    }
}

pub(super) struct ScanResult {
    pub(super) index: ArchiveIndex,
    pub(super) valid_file_len: u64,
}

pub(super) fn scan_archive(
    file: &File,
    path: &Path,
    config: StaticFileConfig,
    file_len: u64,
) -> StaticFileResult<ScanResult> {
    let mut index = ArchiveIndex::empty();
    let mut cursor = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
    let minimum_frame =
        u64::try_from(FRAME_HEADER_LEN + FRAME_FOOTER_LEN).expect("minimum frame length fits u64");
    let mut expected_height = 0u32;

    while cursor < file_len {
        if file_len - cursor < u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64") {
            break;
        }
        let mut header_bytes = [0u8; FRAME_HEADER_LEN];
        read_exact_at(file, cursor, &mut header_bytes)
            .map_err(|source| StaticFileError::io("scan frame header", path, source))?;
        let header = decode_frame_header(&header_bytes, cursor)?;
        validate_frame_shape(header, cursor, config)?;
        if header.frame_len < minimum_frame {
            return Err(StaticFileError::invalid(
                cursor,
                "frame is shorter than framing overhead",
            ));
        }
        let frame_end = cursor
            .checked_add(header.frame_len)
            .ok_or_else(|| StaticFileError::invalid(cursor, "frame end overflow"))?;
        if frame_end > file_len {
            break;
        }
        if header.height != expected_height {
            return Err(StaticFileError::NonContiguous {
                expected: expected_height,
                actual: header.height,
            });
        }

        let mut footer = [0u8; FRAME_FOOTER_LEN];
        let footer_offset = frame_end
            .checked_sub(u64::try_from(FRAME_FOOTER_LEN).expect("footer length fits u64"))
            .ok_or_else(|| StaticFileError::invalid(cursor, "footer offset underflow"))?;
        read_exact_at(file, footer_offset, &mut footer)
            .map_err(|source| StaticFileError::io("scan frame footer", path, source))?;
        validate_footer(&footer, &header_bytes, footer_offset)?;

        let row_index_offset =
            cursor + u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64");
        let mut row_index = vec![
            0u8;
            usize::try_from(header.index_len).map_err(|_| {
                StaticFileError::invalid(cursor, "row-index length does not fit usize")
            })?
        ];
        read_exact_at(file, row_index_offset, &mut row_index)
            .map_err(|source| StaticFileError::io("scan row index", path, source))?;
        let rows = parse_index(header, &row_index, cursor)?;

        let payload_offset = row_index_offset + u64::from(header.index_len);
        let mut compressed = vec![
            0u8;
            usize::try_from(header.compressed_len).map_err(|_| {
                StaticFileError::invalid(cursor, "compressed length does not fit usize")
            })?
        ];
        read_exact_at(file, payload_offset, &mut compressed)
            .map_err(|source| StaticFileError::io("scan compressed payload", path, source))?;
        if let Err(error) = decode_payload(
            header.height,
            header.uncompressed_len,
            header.payload_checksum,
            &compressed,
        ) {
            if frame_end == file_len {
                break;
            }
            return Err(error);
        }

        index.insert_scanned_frame(cursor, header, rows);
        cursor = frame_end;
        expected_height = header.height.checked_add(1).ok_or_else(|| {
            StaticFileError::invalid(cursor, "archive contains data after maximum block height")
        })?;
    }
    index.file_len = cursor;
    Ok(ScanResult {
        index,
        valid_file_len: cursor,
    })
}
