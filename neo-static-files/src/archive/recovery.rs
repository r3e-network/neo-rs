//! Clean-open layout validation and bounded multi-segment suffix recovery.

use super::StaticFileConfig;
use super::factory::StaticFileOpenStats;
use super::index::{
    ArchiveIndex, FrameLocation, ScanMode, ScannedFrame, scan_archive, validate_published_tail,
};
use super::segments::{ArchiveSegment, ArchiveSegments};
use crate::format::FILE_HEADER_LEN;
use crate::{StaticFileError, StaticFileResult};

const INDEX_PUBLICATION_BATCH_FRAMES: usize = 1_024;

pub(super) fn indexed_layout_matches(
    index: &ArchiveIndex,
    segments: &ArchiveSegments,
    config: StaticFileConfig,
    state: super::index::IndexState,
) -> bool {
    let snapshots = segments.snapshots();
    let Some(active_position) = snapshots
        .iter()
        .position(|segment| segment.start_height == state.active_segment_start)
    else {
        return false;
    };
    let active = &snapshots[active_position];
    if active
        .len()
        .map_or(true, |file_len| state.indexed_file_len > file_len)
        || !index_tail_matches(index, active, config, state)
    {
        return false;
    }
    if state.tip.is_none() {
        return state.active_segment_start == 0;
    }

    let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
    for (position, segment) in snapshots.iter().enumerate().take(active_position + 1) {
        if segment.start_height > state.tip.unwrap_or_default() {
            return false;
        }
        if index
            .frame(segment.start_height)
            .ok()
            .flatten()
            .is_none_or(|frame| {
                frame.segment_start != segment.start_height || frame.start != header_len
            })
        {
            return false;
        }
        if position > 0 {
            let previous = &snapshots[position - 1];
            let Some(previous_height) = segment.start_height.checked_sub(1) else {
                return false;
            };
            if index
                .frame(previous_height)
                .ok()
                .flatten()
                .zip(previous.len().ok())
                .is_none_or(|(frame, file_len)| {
                    frame.segment_start != previous.start_height || frame.end != file_len
                })
            {
                return false;
            }
        }
    }
    true
}

pub(super) fn recover_segments(
    index: &ArchiveIndex,
    segments: &mut ArchiveSegments,
    config: StaticFileConfig,
    initial_state: super::index::IndexState,
    rebuild: bool,
    stats: &mut StaticFileOpenStats,
) -> StaticFileResult<()> {
    let scan_mode = if initial_state.tail_recovery_safe {
        ScanMode::RecoverTail
    } else {
        ScanMode::Strict
    };
    let snapshots = segments.snapshots();
    let first_position = if rebuild {
        0
    } else {
        snapshots
            .iter()
            .position(|segment| segment.start_height == initial_state.active_segment_start)
            .ok_or_else(|| {
                StaticFileError::invalid_index("active archive segment disappeared during open")
            })?
    };
    let header_len = u64::try_from(FILE_HEADER_LEN).expect("header length fits u64");
    let mut expected_height = if rebuild {
        0
    } else {
        initial_state.next_height()?
    };
    let mut batch = Vec::<ScannedFrame>::with_capacity(INDEX_PUBLICATION_BATCH_FRAMES);

    for (position, segment) in snapshots.iter().enumerate().skip(first_position) {
        let continuing_active = !rebuild && position == first_position;
        if !continuing_active && segment.start_height != expected_height {
            return Err(StaticFileError::NonContiguous {
                expected: expected_height,
                actual: segment.start_height,
            });
        }
        let file_len = segment.len()?;
        let start = if continuing_active {
            initial_state.indexed_file_len
        } else {
            header_len
        };
        let mode = if position + 1 == snapshots.len() {
            scan_mode
        } else {
            ScanMode::Strict
        };
        let outcome = scan_archive(
            &segment.file,
            &segment.path,
            segment.start_height,
            config,
            file_len,
            start,
            expected_height,
            mode,
            |frame| {
                batch.push(frame);
                if batch.len() == INDEX_PUBLICATION_BATCH_FRAMES {
                    index.publish_frames(&batch)?;
                    batch.clear();
                }
                Ok(())
            },
        )?;
        let scanned = u32::try_from(outcome.frames_scanned)
            .map_err(|_| StaticFileError::invalid_index("segment frame count does not fit u32"))?;
        expected_height = expected_height.checked_add(scanned).ok_or_else(|| {
            StaticFileError::invalid_index("archive height overflow during recovery")
        })?;
        stats.frames_scanned = stats.frames_scanned.saturating_add(outcome.frames_scanned);
        stats.payloads_decoded = stats
            .payloads_decoded
            .saturating_add(outcome.payloads_decoded);
        stats.rows_replayed = stats.rows_replayed.saturating_add(outcome.rows_scanned);

        if outcome.valid_file_len != file_len {
            segment
                .file
                .set_len(outcome.valid_file_len)
                .map_err(|source| {
                    StaticFileError::io("truncate torn segment tail", &segment.path, source)
                })?;
            segment.file.sync_all().map_err(|source| {
                StaticFileError::io("sync recovered segment tail", &segment.path, source)
            })?;
            stats.archive_tail_truncated = true;
        }
    }
    if !batch.is_empty() {
        index.publish_frames(&batch)?;
    }

    let state = index
        .state()
        .ok_or_else(|| StaticFileError::invalid_index("recovery did not initialize the index"))?;
    let orphan_segments = segments
        .snapshots()
        .into_iter()
        .filter(|segment| segment.start_height > state.active_segment_start)
        .collect::<Vec<_>>();
    if orphan_segments.iter().any(|segment| {
        segment.len().map_or(true, |len| {
            len != u64::try_from(FILE_HEADER_LEN).expect("header length fits u64")
        })
    }) {
        return Err(StaticFileError::invalid_index(
            "recovery left a non-empty segment beyond the indexed tip",
        ));
    }
    if !orphan_segments.is_empty() {
        segments.remove_after(state.active_segment_start)?;
        stats.archive_tail_truncated = true;
        stats.segments_retained = u32::try_from(segments.count()).unwrap_or(u32::MAX);
    }
    Ok(())
}

fn index_tail_matches(
    index: &ArchiveIndex,
    segment: &ArchiveSegment,
    config: StaticFileConfig,
    state: super::index::IndexState,
) -> bool {
    if validate_published_tail(&segment.file, &segment.path, config, state).is_err() {
        return false;
    }
    match state.tip {
        Some(height) => index.frame(height).is_ok_and(|location| {
            location
                == Some(FrameLocation {
                    height,
                    segment_start: state.active_segment_start,
                    start: state.last_frame_start,
                    end: state.indexed_file_len,
                })
        }),
        None => true,
    }
}
