//! Forward comparison of the local state-root records against the reference.
//!
//! Both sides are consumed in ascending height order:
//! * local records come from one bounded forward prefix iterator over the
//!   state-root namespace, which sorts numerically by height, so 13.1M heights
//!   cost one sequential scan instead of 13.1M point reads;
//! * reference hashes come from the disk-backed height index built in the first
//!   pass, so unordered reference rows still compare in order.
//!
//! Nothing here can write to the database: the caller hands in a `DB` that was
//! opened read-only (or as a secondary), and this module only calls
//! `get`/`iterator_opt`.

use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use neo_primitives::UInt256;
use neo_state_service::record_schema;
use rocksdb::{DB, Direction, IteratorMode, ReadOptions};

use super::reference_index::{HASH_LEN, HeightHashIndex};

/// Heights to compare plus the mismatch policy.
#[derive(Debug, Clone, Copy)]
pub struct SweepConfig {
    /// First height compared (inclusive).
    pub start: u32,
    /// Last height compared (inclusive).
    pub end: u32,
    /// Stop at the first hash mismatch instead of collecting every mismatch.
    pub stop_on_first_mismatch: bool,
}

impl SweepConfig {
    /// Number of heights the sweep is expected to account for.
    #[must_use]
    pub fn total_heights(&self) -> u64 {
        u64::from(self.end) - u64::from(self.start) + 1
    }
}

/// First differing height, with both hashes in canonical display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mismatch {
    /// Height that differs.
    pub height: u32,
    /// Hash persisted in the local database.
    pub local: [u8; HASH_LEN],
    /// Hash stated by the reference file.
    pub reference: [u8; HASH_LEN],
}

impl Mismatch {
    /// Local hash as a `0x`-prefixed display string.
    #[must_use]
    pub fn local_hex(&self) -> String {
        UInt256::from(self.local).to_string()
    }

    /// Reference hash as a `0x`-prefixed display string.
    #[must_use]
    pub fn reference_hex(&self) -> String {
        UInt256::from(self.reference).to_string()
    }
}

/// Per-height observation used for progress reporting.
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    /// Height just processed.
    pub height: u32,
    /// Heights accounted for so far, relative to `start`.
    pub processed: u64,
}

/// Everything the JSON result and the exit code are derived from.
#[derive(Debug, Clone)]
pub struct SweepOutcome {
    /// Heights present on both sides and therefore actually compared.
    pub compared: u64,
    /// Compared heights whose hashes are equal.
    pub matches: u64,
    /// Compared heights whose hashes differ.
    pub mismatches: u64,
    /// Requested heights with no local root record.
    pub missing_local: u64,
    /// Requested heights with no reference row.
    pub missing_reference: u64,
    /// Local records whose bytes are not a decodable state-root record.
    pub decode_errors: u64,
    /// Lowest local height seen inside the requested range.
    pub first_local_height: Option<u32>,
    /// Highest local height seen inside the requested range.
    pub last_local_height: Option<u32>,
    /// First mismatch, if any.
    pub first_mismatch: Option<Mismatch>,
    /// Lowest requested height without a local record.
    pub first_missing_local: Option<u32>,
    /// Lowest requested height without a reference row.
    pub first_missing_reference: Option<u32>,
    /// Whether the sweep reached `end` without stopping early.
    pub sweep_complete: bool,
    /// Wall time spent in the comparison pass.
    pub elapsed: Duration,
}

impl SweepOutcome {
    /// True only when every requested height was compared and matched.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.sweep_complete
            && self.mismatches == 0
            && self.decode_errors == 0
            && self.missing_local == 0
            && self.missing_reference == 0
            && self.compared > 0
    }
}

/// Streams local records in `[start, end]` and compares them with the index.
///
/// `observe` is called once per accounted height so callers can print windowed
/// and cumulative rates without the sweep owning a clock.
pub fn sweep(
    db: &DB,
    index: &mut HeightHashIndex,
    config: &SweepConfig,
    mut observe: impl FnMut(Observation),
) -> Result<SweepOutcome> {
    let started = Instant::now();
    let prefix = record_schema::state_root_record_prefix();
    let mut lower = prefix.clone();
    lower.extend_from_slice(&config.start.to_be_bytes());

    let mut read_options = ReadOptions::default();
    // Bound the iterator below so a 13.1M-height range does not walk the
    // namespace from height 0, and use total order seek so no prefix extractor
    // can truncate the scan. No upper bound: the loop stops at `end` and on the
    // first key outside the prefix, so the scan can never leak into another
    // namespace.
    read_options.set_iterate_lower_bound(lower.clone());
    read_options.set_total_order_seek(true);
    read_options.set_verify_checksums(true);
    read_options.fill_cache(false);
    read_options.set_readahead_size(4 << 20);

    let iterator = db.iterator_opt(
        IteratorMode::From(lower.as_slice(), Direction::Forward),
        read_options,
    );

    let mut outcome = SweepOutcome {
        compared: 0,
        matches: 0,
        mismatches: 0,
        missing_local: 0,
        missing_reference: 0,
        decode_errors: 0,
        first_local_height: None,
        last_local_height: None,
        first_mismatch: None,
        first_missing_local: None,
        first_missing_reference: None,
        sweep_complete: false,
        elapsed: Duration::ZERO,
    };
    let mut expected_next = u64::from(config.start);
    let mut stopped_early = false;

    for entry in iterator {
        let (key, value) = entry.context("failed to read a local state-root record")?;
        if !key.starts_with(&prefix) {
            break;
        }
        if key.len() != prefix.len() + 4 {
            outcome.decode_errors += 1;
            continue;
        }
        let mut raw_height = [0u8; 4];
        raw_height.copy_from_slice(&key[prefix.len()..]);
        let height = u32::from_be_bytes(raw_height);
        if height > config.end {
            break;
        }
        let height64 = u64::from(height);
        if height64 > expected_next {
            account_gap(
                index,
                &mut outcome,
                expected_next,
                height64,
                &mut observe,
                config,
            )?;
        }

        outcome.first_local_height.get_or_insert(height);
        outcome.last_local_height = Some(height);

        // Decode with the canonical live-service decoder: a record this tool
        // cannot decode is a decode failure, never a silent hash comparison.
        match record_schema::decode_state_root_record(height, &value) {
            None => outcome.decode_errors += 1,
            Some(root) => {
                let local = root.root_hash.to_array();
                match index
                    .get(height)
                    .with_context(|| format!("failed to look up reference height {height}"))?
                {
                    None => {
                        outcome.missing_reference += 1;
                        outcome.first_missing_reference.get_or_insert(height);
                    }
                    Some(reference) => {
                        outcome.compared += 1;
                        if reference == local {
                            outcome.matches += 1;
                        } else {
                            outcome.mismatches += 1;
                            outcome.first_mismatch.get_or_insert(Mismatch {
                                height,
                                local,
                                reference,
                            });
                            if config.stop_on_first_mismatch {
                                stopped_early = true;
                            }
                        }
                    }
                }
            }
        }

        expected_next = height64 + 1;
        observe(Observation {
            height,
            processed: expected_next - u64::from(config.start),
        });
        if stopped_early {
            break;
        }
    }

    if !stopped_early && expected_next <= u64::from(config.end) {
        account_gap(
            index,
            &mut outcome,
            expected_next,
            u64::from(config.end) + 1,
            &mut observe,
            config,
        )?;
    }

    outcome.sweep_complete = !stopped_early;
    outcome.elapsed = started.elapsed();
    Ok(outcome)
}

/// Accounts for requested heights in `[from, to)` that have no local record.
fn account_gap(
    index: &mut HeightHashIndex,
    outcome: &mut SweepOutcome,
    from: u64,
    to: u64,
    observe: &mut impl FnMut(Observation),
    config: &SweepConfig,
) -> Result<()> {
    let mut height = from;
    while height < to {
        let height32 = u32::try_from(height).context("height exceeds u32")?;
        outcome.missing_local += 1;
        outcome.first_missing_local.get_or_insert(height32);
        if index
            .get(height32)
            .with_context(|| format!("failed to look up reference height {height32}"))?
            .is_none()
        {
            outcome.missing_reference += 1;
            outcome.first_missing_reference.get_or_insert(height32);
        }
        observe(Observation {
            height: height32,
            processed: height + 1 - u64::from(config.start),
        });
        height += 1;
    }
    Ok(())
}

/// Reads the durable local/validated cursors from an opened database.
pub fn read_cursors(db: &DB) -> Result<(Option<u32>, Option<u32>)> {
    let local = read_cursor(db, &record_schema::local_root_cursor_key().to_array())?;
    let validated = read_cursor(db, &record_schema::validated_root_cursor_key().to_array())?;
    Ok((local, validated))
}

fn read_cursor(db: &DB, key: &[u8]) -> Result<Option<u32>> {
    let Some(value) = db.get(key).context("failed to read a state-root cursor")? else {
        return Ok(None);
    };
    Ok(record_schema::decode_cursor(&value))
}
