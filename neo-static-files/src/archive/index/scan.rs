//! Sequential suffix recovery and opt-in full archive verification.

use std::fs::File;
use std::path::Path;

use super::model::{FrameLocation, IndexState, ScannedFrame};
use crate::archive::StaticFileConfig;
use crate::archive::io::read_exact_at;
use crate::format::{
    FILE_HEADER_LEN, FRAME_FOOTER_LEN, FRAME_HEADER_LEN, decode_frame_header, decode_payload,
    parse_index, validate_footer, validate_frame_shape,
};
use crate::{StaticFileError, StaticFileResult};

/// Whether an incomplete or corrupt final payload is recoverable as unpublished tail data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ScanMode {
    RecoverTail,
    Strict,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ScanOutcome {
    pub(crate) valid_file_len: u64,
    pub(crate) frames_scanned: u64,
    pub(crate) payloads_decoded: u64,
    pub(crate) rows_scanned: u64,
}

pub(crate) fn scan_archive<F>(
    file: &File,
    path: &Path,
    segment_start: u32,
    config: StaticFileConfig,
    file_len: u64,
    start: u64,
    mut expected_height: u32,
    mode: ScanMode,
    mut on_frame: F,
) -> StaticFileResult<ScanOutcome>
where
    F: FnMut(ScannedFrame) -> StaticFileResult<()>,
{
    let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
    if start < header_len || start > file_len {
        return Err(StaticFileError::invalid(
            start,
            "archive scan start is outside the file",
        ));
    }

    let mut cursor = start;
    let mut outcome = ScanOutcome {
        valid_file_len: start,
        ..ScanOutcome::default()
    };
    let frame_header_len = u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64");

    while cursor < file_len {
        if file_len - cursor < frame_header_len {
            if mode == ScanMode::Strict {
                return Err(StaticFileError::invalid(cursor, "truncated frame header"));
            }
            break;
        }
        let mut header_bytes = [0u8; FRAME_HEADER_LEN];
        read_exact_at(file, cursor, &mut header_bytes)
            .map_err(|source| StaticFileError::io("scan frame header", path, source))?;
        let header = decode_frame_header(&header_bytes, cursor)?;
        validate_frame_shape(header, cursor, config)?;
        let frame_end = cursor
            .checked_add(header.frame_len)
            .ok_or_else(|| StaticFileError::invalid(cursor, "frame end overflow"))?;
        if frame_end > file_len {
            if mode == ScanMode::Strict {
                return Err(StaticFileError::invalid(cursor, "truncated frame"));
            }
            break;
        }
        if header.height != expected_height {
            return Err(StaticFileError::NonContiguous {
                expected: expected_height,
                actual: header.height,
            });
        }

        let footer_offset = frame_end
            .checked_sub(u64::try_from(FRAME_FOOTER_LEN).expect("footer length fits u64"))
            .ok_or_else(|| StaticFileError::invalid(cursor, "footer offset underflow"))?;
        let mut footer = [0u8; FRAME_FOOTER_LEN];
        read_exact_at(file, footer_offset, &mut footer)
            .map_err(|source| StaticFileError::io("scan frame footer", path, source))?;
        validate_footer(&footer, &header_bytes, footer_offset)?;

        let row_index_offset = cursor + frame_header_len;
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
            if mode == ScanMode::RecoverTail && frame_end == file_len {
                break;
            }
            return Err(error);
        }

        outcome.frames_scanned = outcome.frames_scanned.saturating_add(1);
        outcome.payloads_decoded = outcome.payloads_decoded.saturating_add(1);
        outcome.rows_scanned = outcome
            .rows_scanned
            .saturating_add(u64::try_from(rows.len()).unwrap_or(u64::MAX));
        on_frame(ScannedFrame {
            segment_start,
            offset: cursor,
            header,
            rows,
        })?;
        cursor = frame_end;
        outcome.valid_file_len = cursor;
        expected_height = header.height.checked_add(1).ok_or_else(|| {
            StaticFileError::invalid(cursor, "archive contains data after maximum block height")
        })?;
    }
    Ok(outcome)
}

pub(crate) fn validate_published_tail(
    file: &File,
    path: &Path,
    config: StaticFileConfig,
    state: IndexState,
) -> StaticFileResult<()> {
    let Some(tip) = state.tip else {
        if state.indexed_file_len != u64::try_from(FILE_HEADER_LEN).expect("header length fits u64")
        {
            return Err(StaticFileError::invalid_index(
                "empty index points beyond the archive header",
            ));
        }
        return Ok(());
    };

    let mut header_bytes = [0u8; FRAME_HEADER_LEN];
    read_exact_at(file, state.last_frame_start, &mut header_bytes)
        .map_err(|source| StaticFileError::io("read published tail header", path, source))?;
    let header = decode_frame_header(&header_bytes, state.last_frame_start)?;
    validate_frame_shape(header, state.last_frame_start, config)?;
    if header.height != tip
        || state
            .last_frame_start
            .checked_add(header.frame_len)
            .is_none_or(|end| end != state.indexed_file_len)
    {
        return Err(StaticFileError::invalid_index(
            "published tail does not match index state",
        ));
    }
    let footer_offset =
        state.indexed_file_len - u64::try_from(FRAME_FOOTER_LEN).expect("footer length fits u64");
    let mut footer = [0u8; FRAME_FOOTER_LEN];
    read_exact_at(file, footer_offset, &mut footer)
        .map_err(|source| StaticFileError::io("read published tail footer", path, source))?;
    validate_footer(&footer, &header_bytes, footer_offset)
}

pub(crate) fn read_frame_index(
    file: &File,
    path: &Path,
    segment_start: u32,
    config: StaticFileConfig,
    location: FrameLocation,
) -> StaticFileResult<ScannedFrame> {
    let mut header_bytes = [0u8; FRAME_HEADER_LEN];
    read_exact_at(file, location.start, &mut header_bytes)
        .map_err(|source| StaticFileError::io("read frame header", path, source))?;
    let header = decode_frame_header(&header_bytes, location.start)?;
    validate_frame_shape(header, location.start, config)?;
    if header.height != location.height
        || location.segment_start != segment_start
        || location
            .start
            .checked_add(header.frame_len)
            .is_none_or(|end| end != location.end)
    {
        return Err(StaticFileError::invalid_index(
            "frame directory disagrees with archive framing",
        ));
    }
    let footer_offset =
        location.end - u64::try_from(FRAME_FOOTER_LEN).expect("footer length fits u64");
    let mut footer = [0u8; FRAME_FOOTER_LEN];
    read_exact_at(file, footer_offset, &mut footer)
        .map_err(|source| StaticFileError::io("read frame footer", path, source))?;
    validate_footer(&footer, &header_bytes, footer_offset)?;

    let index_offset =
        location.start + u64::try_from(FRAME_HEADER_LEN).expect("header length fits u64");
    let mut index = vec![
        0u8;
        usize::try_from(header.index_len).map_err(|_| {
            StaticFileError::invalid(location.start, "row-index length does not fit usize")
        })?
    ];
    read_exact_at(file, index_offset, &mut index)
        .map_err(|source| StaticFileError::io("read frame row index", path, source))?;
    let rows = parse_index(header, &index, location.start)?;
    Ok(ScannedFrame {
        segment_start,
        offset: location.start,
        header,
        rows,
    })
}
