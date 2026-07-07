//! Native stack-result decoders for read-only RPC probes.
//!
//! VM invocation returns generic stack items. This module owns the NEO-specific
//! result shapes expected by the RPC projection layer.

use neo_error::{CoreError, CoreResult};
use neo_vm::StackItem;
use num_bigint::BigInt;

/// Decodes a stack array whose elements are byte strings.
pub(super) fn stack_array_of_bytes(item: &StackItem) -> CoreResult<Vec<Vec<u8>>> {
    let entries = item
        .as_array()
        .map_err(|err| CoreError::other(err.to_string()))?;
    entries
        .iter()
        .map(|entry| {
            entry
                .as_bytes()
                .map_err(|err| CoreError::other(err.to_string()))
        })
        .collect()
}

/// Decodes `NEO.getCandidates()` entries as `(public_key, votes)` pairs.
pub(super) fn candidate_entries(item: &StackItem) -> CoreResult<Vec<(Vec<u8>, BigInt)>> {
    let entries = item
        .as_array()
        .map_err(|err| CoreError::other(err.to_string()))?;
    let mut candidates = Vec::with_capacity(entries.len());
    for entry in entries {
        let fields = entry
            .as_array()
            .map_err(|err| CoreError::other(err.to_string()))?;
        if fields.len() != 2 {
            return Err(CoreError::other(format!(
                "getCandidates entry has {} fields, expected 2",
                fields.len()
            )));
        }
        let pubkey = fields[0]
            .as_bytes()
            .map_err(|err| CoreError::other(err.to_string()))?;
        let votes = fields[1]
            .as_int()
            .map_err(|err| CoreError::other(err.to_string()))?;
        candidates.push((pubkey, votes));
    }
    Ok(candidates)
}
