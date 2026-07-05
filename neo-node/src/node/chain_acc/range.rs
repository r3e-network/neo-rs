//! Range, resume, and continuity validation for `chain.acc` imports.
//!
//! These helpers keep the stream reader focused on I/O and import dispatch.
//! They validate expected file ranges, local-ledger resume points, count-only
//! stop heights, and previous-hash continuity before any batch reaches the
//! blockchain service.

use neo_payloads::block::Block;
use neo_primitives::UInt256;

use super::{ChainAccExpectedRange, LocalLedgerTip};

pub(super) fn validate_chain_acc_count(
    count: usize,
    range: ChainAccExpectedRange,
) -> anyhow::Result<()> {
    let expected_count = expected_chain_acc_count(range)?;
    if count != expected_count {
        anyhow::bail!(
            "chain.acc count mismatch for expected range {}..={}: expected {expected_count} blocks, file has {count}",
            range.start_height,
            range.end_height
        );
    }
    Ok(())
}

pub(super) fn bounded_chain_acc_import_range(
    expected_range: Option<ChainAccExpectedRange>,
    header_start_height: Option<u32>,
    stop_at_height: Option<u32>,
) -> Option<ChainAccExpectedRange> {
    if let Some(range) = expected_range {
        let Some(stop_at_height) = stop_at_height else {
            return Some(range);
        };
        if stop_at_height < range.start_height {
            return None;
        }
        return Some(ChainAccExpectedRange {
            start_height: range.start_height,
            end_height: range.end_height.min(stop_at_height),
        });
    }

    let start_height = header_start_height?;
    let stop_at_height = stop_at_height?;
    if stop_at_height < start_height {
        return None;
    }
    Some(ChainAccExpectedRange {
        start_height,
        end_height: stop_at_height,
    })
}

pub(super) fn resume_chain_acc_import_range(
    import_range: Option<ChainAccExpectedRange>,
    local_tip: Option<&LocalLedgerTip>,
) -> anyhow::Result<Option<ChainAccExpectedRange>> {
    let Some(range) = import_range else {
        return Ok(None);
    };
    let Some(local_tip) = local_tip else {
        return Ok(Some(range));
    };

    if local_tip.height >= range.end_height {
        return Ok(None);
    }
    if local_tip.height < range.start_height {
        let Some(expected_previous_height) = range.start_height.checked_sub(1) else {
            return Ok(Some(range));
        };
        if local_tip.height != expected_previous_height {
            anyhow::bail!(
                "chain.acc expected range {}..={} requires local ledger tip at height {expected_previous_height} or inside the range, got {}",
                range.start_height,
                range.end_height,
                local_tip.height
            );
        }
        return Ok(Some(range));
    }

    let start_height = local_tip.height.checked_add(1).ok_or_else(|| {
        anyhow::anyhow!(
            "local ledger tip height {} cannot be advanced for chain.acc resume",
            local_tip.height
        )
    })?;
    Ok(Some(ChainAccExpectedRange {
        start_height,
        end_height: range.end_height,
    }))
}

pub(super) fn chain_acc_import_record_count(
    file_count: usize,
    expected_range: Option<ChainAccExpectedRange>,
    import_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
) -> anyhow::Result<usize> {
    match (expected_range, import_range) {
        (Some(_), Some(range)) => expected_chain_acc_count(range),
        (Some(_), None) => Ok(0),
        (None, Some(range)) => expected_chain_acc_count(range).map(|count| count.min(file_count)),
        (None, None) if stop_at_height.is_some() => Ok(file_count),
        (None, _) => Ok(file_count),
    }
}

pub(super) fn chain_acc_records_to_skip(
    file_count: usize,
    expected_range: Option<ChainAccExpectedRange>,
    header_start_height: Option<u32>,
    import_range: Option<ChainAccExpectedRange>,
) -> anyhow::Result<usize> {
    let Some(import_range) = import_range else {
        return Ok(0);
    };
    let Some(file_start_height) = expected_range
        .map(|range| range.start_height)
        .or(header_start_height)
    else {
        return Ok(0);
    };
    let skip = import_range
        .start_height
        .checked_sub(file_start_height)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chain.acc import start {} is before file start {file_start_height}",
                import_range.start_height
            )
        })? as usize;
    if skip > file_count {
        anyhow::bail!(
            "chain.acc import start {} skips {skip} records, but file has only {file_count} records",
            import_range.start_height
        );
    }
    Ok(skip)
}

pub(super) fn count_only_stop_height_reached(
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    block_height: u32,
) -> bool {
    expected_range.is_none() && stop_at_height.is_some_and(|target| block_height >= target)
}

pub(super) fn count_only_stop_height_exceeded(
    expected_range: Option<ChainAccExpectedRange>,
    stop_at_height: Option<u32>,
    block_height: u32,
) -> bool {
    expected_range.is_none() && stop_at_height.is_some_and(|target| block_height > target)
}

pub(super) fn validate_chain_acc_block_height(
    record: usize,
    height: u32,
    header_start_height: Option<u32>,
    expected_range: Option<ChainAccExpectedRange>,
    expected_count: Option<usize>,
    previous_height: &mut Option<u32>,
) -> anyhow::Result<()> {
    if record == 0 {
        if let Some(expected_first_height) = expected_range
            .map(|range| range.start_height)
            .or(header_start_height)
        {
            if height != expected_first_height {
                anyhow::bail!(
                    "chain.acc first block height mismatch: expected {expected_first_height}, got {height}"
                );
            }
        }
    } else if let Some(previous) = previous_height {
        if height != previous.saturating_add(1) {
            anyhow::bail!(
                "chain.acc block heights are not contiguous at record {record}: expected {}, got {height}",
                previous.saturating_add(1)
            );
        }
    }

    if let (Some(range), Some(expected_count)) = (expected_range, expected_count) {
        if record + 1 == expected_count && height != range.end_height {
            anyhow::bail!(
                "chain.acc last block height mismatch: expected {}, got {height}",
                range.end_height
            );
        }
    }

    *previous_height = Some(height);
    Ok(())
}

pub(super) fn expected_chain_acc_first_prev_hash(
    expected_range: Option<ChainAccExpectedRange>,
    local_tip: Option<&LocalLedgerTip>,
) -> anyhow::Result<Option<UInt256>> {
    let Some(range) = expected_range else {
        return Ok(None);
    };
    if range.start_height == 0 {
        return Ok(None);
    }
    let Some(local_tip) = local_tip else {
        anyhow::bail!(
            "chain.acc partial expected range {}..={} requires local storage for previous hash validation",
            range.start_height,
            range.end_height
        );
    };
    let expected_previous_height = range.start_height.checked_sub(1).ok_or_else(|| {
        anyhow::anyhow!(
            "chain.acc expected range is invalid: {}..={}",
            range.start_height,
            range.end_height
        )
    })?;
    if local_tip.height != expected_previous_height {
        anyhow::bail!(
            "chain.acc partial expected range {}..={} requires local ledger tip at height {expected_previous_height}, got {}",
            range.start_height,
            range.end_height,
            local_tip.height
        );
    }
    Ok(Some(local_tip.hash))
}

pub(super) fn validate_chain_acc_first_prev_hash(
    record: usize,
    block: &Block,
    expected_prev_hash: Option<&UInt256>,
) -> anyhow::Result<()> {
    let Some(expected_prev_hash) = expected_prev_hash else {
        return Ok(());
    };
    if record != 0 {
        return Ok(());
    }
    if block.prev_hash() != expected_prev_hash {
        anyhow::bail!(
            "chain.acc previous hash mismatch at first imported block {}: expected local tip hash {}, got {}",
            block.index(),
            expected_prev_hash,
            block.prev_hash()
        );
    }
    Ok(())
}

pub(super) fn validate_chain_acc_internal_prev_hash(
    record: usize,
    block: &Block,
    previous_hash: Option<&UInt256>,
) -> anyhow::Result<()> {
    let Some(previous_hash) = previous_hash else {
        return Ok(());
    };
    if block.prev_hash() != previous_hash {
        anyhow::bail!(
            "chain.acc previous hash mismatch at record {record}, block {}: expected previous block hash {}, got {}",
            block.index(),
            previous_hash,
            block.prev_hash()
        );
    }
    Ok(())
}

pub(super) fn expected_chain_acc_count(range: ChainAccExpectedRange) -> anyhow::Result<usize> {
    Ok(range
        .end_height
        .checked_sub(range.start_height)
        .and_then(|span| span.checked_add(1))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chain.acc expected range is invalid: {}..={}",
                range.start_height,
                range.end_height
            )
        })? as usize)
}
