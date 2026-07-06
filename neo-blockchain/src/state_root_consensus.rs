//! Active signed-StateRoot consensus core (StateValidators).
//!
//! Mirrors the vote/aggregate flow of C# `Neo.Plugins.StateService`:
//! 1. Each designated **StateValidator** signs the sign-data of a locally
//!    computed [`neo_state_service::StateRoot`] (`network || Hash`) and
//!    broadcasts a `Vote`.
//! 2. Nodes collect votes per root index. Once `M = bft_threshold(N)` valid
//!    votes exist, the signatures are aggregated into the StateValidators
//!    `M`-of-`N` multisig witness — producing a **network-signed** `StateRoot`
//!    that [`crate::verify_state_root_with_native_provider`] accepts.
//!
//! This module is the deterministic state-machine core. The node layer feeds it
//! inbound votes (from the `StateService` extensible-payload category) and
//! broadcasts the local vote and the aggregated signed root.

use std::collections::HashMap;

use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_payloads::Witness;
use neo_state_service::StateRoot;
use neo_vm::script_builder::{RedeemScript, ScriptBuilder};

/// Signs a state root's sign-data (`network || Hash`) with a StateValidator's
/// secp256r1 key, producing the 64-byte signature carried in a `Vote`.
pub fn sign_state_root(
    state_root: &mut StateRoot,
    private_key: &[u8; 32],
    network: u32,
) -> Option<[u8; 64]> {
    let sign_data = state_root.get_sign_data(network);
    Secp256r1Crypto::sign(&sign_data, private_key).ok()
}

/// Validates a vote signature against the designated validator's key over the
/// root's sign-data. Returns `false` for an out-of-range index, a wrong-length
/// signature, or a signature that does not verify.
pub fn validate_state_root_vote(
    state_root: &mut StateRoot,
    validator_index: usize,
    signature: &[u8],
    validators: &[ECPoint],
    network: u32,
) -> bool {
    let Some(pubkey) = validators.get(validator_index) else {
        return false;
    };
    let Ok(sig) = <[u8; 64]>::try_from(signature) else {
        return false;
    };
    let sign_data = state_root.get_sign_data(network);
    Secp256r1Crypto::verify(&sign_data, &sig, pubkey.as_bytes()).unwrap_or(false)
}

/// Aggregates collected votes (`validator_index -> 64-byte signature`) into the
/// StateValidators `M`-of-`N` multisig witness, if at least `M` votes exist. The
/// invocation script pushes the `M` signatures in the verification script's
/// canonical (sorted-key) order, matching C# `CheckMultisig`.
pub fn aggregate_state_root_witness(
    votes: &HashMap<usize, Vec<u8>>,
    validators: &[ECPoint],
) -> Option<Witness> {
    if validators.is_empty() {
        return None;
    }
    let m = RedeemScript::bft_threshold(validators.len());
    if votes.len() < m {
        return None;
    }
    let verification_script =
        RedeemScript::multi_sig_redeem_script_from_points(m, validators).ok()?;

    // Push signatures in the verification script's sorted-key order (C#
    // CheckMultisig walks signatures and keys in lockstep).
    let mut sorted: Vec<(usize, &ECPoint)> = validators.iter().enumerate().collect();
    sorted.sort_by(|a, b| a.1.cmp(b.1));

    let mut builder = ScriptBuilder::new();
    let mut pushed = 0usize;
    for (index, _key) in sorted {
        if pushed >= m {
            break;
        }
        if let Some(signature) = votes.get(&index) {
            builder.emit_push(signature);
            pushed += 1;
        }
    }
    if pushed < m {
        return None;
    }
    Some(Witness::new_with_scripts(
        builder.to_array(),
        verification_script,
    ))
}

/// Collects StateRoot votes per root index and aggregates them into a signed
/// root once `M` valid votes exist. The node layer holds one of these, feeding
/// it inbound votes and broadcasting the returned signed root.
#[derive(Default)]
pub struct StateRootVoteCollector {
    votes: HashMap<u32, HashMap<usize, Vec<u8>>>,
}

impl StateRootVoteCollector {
    /// Creates an empty vote collector.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a vote after validating its signature. Returns the aggregated,
    /// network-signed [`StateRoot`] once `M` valid votes for `state_root` have
    /// accumulated (and on every call thereafter, since the pool retains them).
    /// A vote whose signature does not verify is dropped and returns `None`.
    pub fn add_vote(
        &mut self,
        state_root: &mut StateRoot,
        validator_index: usize,
        signature: Vec<u8>,
        validators: &[ECPoint],
        network: u32,
    ) -> Option<StateRoot> {
        if !validate_state_root_vote(state_root, validator_index, &signature, validators, network) {
            return None;
        }
        let entry = self.votes.entry(state_root.index()).or_default();
        entry.insert(validator_index, signature);
        let witness = aggregate_state_root_witness(entry, validators)?;
        Some(state_root.clone().with_witness(witness))
    }

    /// Drops vote pools for roots below `index` (called after a signed root is
    /// finalized/persisted) to bound memory.
    pub fn prune_below(&mut self, index: u32) {
        self.votes.retain(|root_index, _| *root_index >= index);
    }
}

#[cfg(test)]
#[path = "tests/state_root_consensus.rs"]
mod tests;
