use anyhow::{Context, Result, bail, ensure};
use neo_crypto::Sha256Hasher;
use neo_state_packs::checkpoint::PackCheckpoint;
use neo_state_packs::{CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PACK_KEY_BYTES, PackStore};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{RawReadOnlyStore, Store};

use super::{
    AUTHORITY_LOOKUP_BATCH_VALUE_BYTES, AUTHORITY_LOOKUP_MAX_VALUE_BYTES, BATCH, STATE_NODE_PREFIX,
    XorShift64,
};

const MAX_CHECKPOINT_SAMPLES: usize = 1_000_000;

pub(super) fn compare_checkpoint_nodes(
    state_store: &RuntimeStore,
    pack: &PackStore,
    checkpoint: &PackCheckpoint,
    expected_digest: [u8; 32],
    samples: usize,
    walk_cap: u64,
    full_scan: bool,
) -> Result<()> {
    ensure!(
        samples <= MAX_CHECKPOINT_SAMPLES,
        "checkpoint sample count exceeds the hard limit of {MAX_CHECKPOINT_SAMPLES}"
    );
    let mut hasher = Sha256Hasher::new();
    hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
    let mut rng = XorShift64(0x9E37_79B9_7F4A_7C15);
    let mut reservoir = Vec::<[u8; PACK_KEY_BYTES]>::new();
    reservoir
        .try_reserve_exact(samples)
        .context("checkpoint sample key allocation exceeds available memory")?;
    let mut total_keys = 0u64;
    let mut total_value_bytes = 0u64;
    let sample_limit = u64::try_from(samples).expect("checkpoint sample limit fits u64");
    let maximum = (!full_scan).then_some(walk_cap);
    state_store.visit_raw_entries_with_prefix(&[STATE_NODE_PREFIX], maximum, |key, value| {
        if key.len() != PACK_KEY_BYTES || key.first() != Some(&STATE_NODE_PREFIX) {
            return Err(neo_storage::StorageError::invalid_operation(
                "StateService node scan returned a malformed key",
            ));
        }
        if value.len() > AUTHORITY_LOOKUP_MAX_VALUE_BYTES {
            return Err(neo_storage::StorageError::invalid_operation(format!(
                "StateService node value has {} bytes, exceeding the verifier limit of {AUTHORITY_LOOKUP_MAX_VALUE_BYTES} bytes",
                value.len()
            )));
        }
        let key: [u8; PACK_KEY_BYTES] = key.try_into().expect("validated pack key");
        hasher.update(&(key.len() as u32).to_le_bytes());
        hasher.update(&key);
        hasher.update(&(value.len() as u64).to_le_bytes());
        hasher.update(value);
        total_value_bytes = total_value_bytes
            .checked_add(u64::try_from(value.len()).expect("value length fits u64"))
            .ok_or_else(|| {
                neo_storage::StorageError::invalid_operation(
                    "StateService node value byte count overflows",
                )
            })?;
        let next_total_keys = total_keys.checked_add(1).ok_or_else(|| {
            neo_storage::StorageError::invalid_operation("StateService node row count overflows")
        })?;
        if total_keys < sample_limit {
            reservoir.push(key);
        } else if samples != 0 {
            let index = rng.next() % next_total_keys;
            if index < sample_limit {
                reservoir[usize::try_from(index).expect("sample index fits usize")] = key;
            }
        }
        total_keys = next_total_keys;
        Ok(())
    })?;
    if full_scan {
        ensure!(
            total_keys == checkpoint.rows && total_value_bytes == checkpoint.value_bytes,
            "full MDBX node geometry differs from checkpoint.json"
        );
        ensure!(
            hasher.finalize() == expected_digest,
            "full MDBX node namespace digest differs from checkpoint.json"
        );
        println!(
            "full MDBX evidence: rows={} value_bytes={} digest=0x{}",
            total_keys,
            total_value_bytes,
            hex::encode(expected_digest),
        );
    } else if total_keys >= walk_cap {
        println!("walk capped at {walk_cap} keys (prefix-bounded sample)");
    }
    println!(
        "mdbx checkpoint node keys: {total_keys}; sampled: {}",
        reservoir.len()
    );

    reservoir.sort_unstable();
    let state_snapshot = state_store.snapshot();
    let mut matched = 0u64;
    let mut first_mismatch = None;
    let mut start = 0usize;
    while start < reservoir.len() {
        let mut expected_values = Vec::new();
        let mut expected_bytes = 0usize;
        let mut end = start;
        while end < reservoir.len() && end - start < BATCH {
            let expected = state_snapshot
                .try_get_bytes_result(&reservoir[end])?
                .context("sampled MDBX checkpoint key disappeared from its frozen snapshot")?;
            ensure!(
                expected.len() <= AUTHORITY_LOOKUP_MAX_VALUE_BYTES,
                "one checkpoint sample value exceeds the lookup byte budget"
            );
            if !expected_values.is_empty()
                && expected_bytes
                    .checked_add(expected.len())
                    .is_none_or(|bytes| bytes > AUTHORITY_LOOKUP_BATCH_VALUE_BYTES)
            {
                break;
            }
            expected_bytes = expected_bytes
                .checked_add(expected.len())
                .context("checkpoint sample byte count overflows")?;
            expected_values.push(expected);
            end += 1;
        }
        ensure!(end > start, "checkpoint sample batching made no progress");
        let keys = &reservoir[start..end];
        let values = pack.get_many_sorted_bounded(
            keys,
            AUTHORITY_LOOKUP_MAX_VALUE_BYTES as u64,
            AUTHORITY_LOOKUP_BATCH_VALUE_BYTES as u64,
        )?;
        ensure!(
            values.len() == keys.len(),
            "pack batch lookup returned a different number of results"
        );
        for ((key, expected), actual) in keys.iter().zip(&expected_values).zip(values) {
            if actual.as_deref() == Some(expected.as_slice()) {
                matched = matched.saturating_add(1);
            } else if first_mismatch.is_none() {
                first_mismatch = Some((*key, expected.clone(), actual.unwrap_or_default()));
            }
        }
        pack.reclaim_random_lookup_pages()?;
        start = end;
    }
    println!("checkpoint sample matched: {matched}");
    if let Some((key, expected, actual)) = first_mismatch {
        bail!(
            "checkpoint differs from MDBX at 0x{}: mdbx {} bytes (0x{}...), pack {} bytes (0x{}...)",
            hex::encode(key),
            expected.len(),
            hex::encode(&expected[..expected.len().min(16)]),
            actual.len(),
            hex::encode(&actual[..actual.len().min(16)]),
        );
    }
    Ok(())
}
