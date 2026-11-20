//! Streaming reader for the reference JSONL, projected into the height index.
//!
//! The reference is a newline-delimited dump of `{"height", "roothash"}` rows.
//! Its record order is not guaranteed (the production file was observed to
//! contain every height in unordered order), so rows are never buffered in
//! memory: each one is written into the height-addressed slot file and only
//! counters survive. Memory therefore stays constant for 13.15M rows.
//!
//! Heights outside `[start, end]` are counted and dropped so a reference that
//! covers more than the requested range does not inflate the temp index.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use neo_primitives::UInt256;
use serde::Deserialize;

use super::reference_index::HeightHashIndex;

const READ_BUFFER_BYTES: usize = 1 << 20;

#[derive(Debug, Deserialize)]
struct Row {
    #[serde(alias = "index", alias = "block_height", alias = "blockHeight")]
    height: u32,
    #[serde(
        alias = "root",
        alias = "state_root",
        alias = "stateRoot",
        alias = "hash"
    )]
    roothash: String,
}

/// Counters gathered while projecting the reference file.
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Non-empty lines parsed.
    pub rows_read: u64,
    /// Rows written into the height index.
    pub rows_indexed: u64,
    /// Rows below the requested start height (ignored).
    pub rows_below_start: u64,
    /// Rows above the requested end height (ignored).
    pub rows_above_end: u64,
    /// True when the file was non-decreasing as read.
    pub observed_ascending: bool,
    /// Highest height that was indexed.
    pub max_indexed: Option<u32>,
    /// Line count of the file.
    pub lines: u64,
}

/// Streams `path` into `index`, keeping only heights inside `[start, end]`.
///
/// A `None` end means "index every height at or above `start`"; the caller then
/// derives the end height from [`ScanStats::max_indexed`].
pub fn build(
    path: &Path,
    start: u32,
    end: Option<u32>,
    index: &mut HeightHashIndex,
    mut observe: impl FnMut(u64),
) -> Result<ScanStats> {
    let file = File::open(path)
        .with_context(|| format!("failed to open reference file {}", path.display()))?;
    let mut reader = BufReader::with_capacity(READ_BUFFER_BYTES, file);
    let mut line = String::new();
    let mut stats = ScanStats {
        observed_ascending: true,
        ..ScanStats::default()
    };
    let mut previous: Option<u32> = None;

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        stats.lines += 1;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let row: Row = serde_json::from_str(trimmed).with_context(|| {
            format!(
                "{}:{}: reference row is not {{\"height\",\"roothash\"}} JSON",
                path.display(),
                stats.lines
            )
        })?;
        stats.rows_read += 1;
        if previous.is_some_and(|height| row.height < height) {
            stats.observed_ascending = false;
        }
        previous = Some(row.height);

        if row.height < start {
            stats.rows_below_start += 1;
            observe(stats.rows_read);
            continue;
        }
        if end.is_some_and(|end| row.height > end) {
            stats.rows_above_end += 1;
            observe(stats.rows_read);
            continue;
        }

        let hash = UInt256::from_str(&row.roothash).with_context(|| {
            format!(
                "{}:{}: root hash {:?} is not a 32-byte 0x-prefixed hex string",
                path.display(),
                stats.lines,
                row.roothash
            )
        })?;
        index
            .insert(row.height, &hash.to_array())
            .with_context(|| format!("failed to index reference height {}", row.height))?;
        stats.rows_indexed += 1;
        stats.max_indexed = Some(stats.max_indexed.map_or(row.height, |h| h.max(row.height)));
        observe(stats.rows_read);
    }

    Ok(stats)
}
