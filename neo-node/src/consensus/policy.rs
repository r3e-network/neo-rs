use super::*;

use neo_consensus::{ProposalLimits, ProposalPolicy};
use neo_io::Serializable;
use neo_native_contracts::native::LedgerContract;
use neo_native_contracts::native::ledger_contract::HashOrIndex;
use neo_protocol::payloads::{Header, TransactionAttribute, Witness};
use neo_vm::{OpCode, ScriptBuilder};
use tracing::warn;

pub(super) fn fetch_block_timestamp(system: &neo_system::NeoSystem, index: u32) -> Option<u64> {
    let store_cache = system.store_cache();
    let ledger = LedgerContract::new();
    ledger
        .get_block(&store_cache, HashOrIndex::Index(index))
        .ok()
        .flatten()
        .map(|block| block.header.timestamp)
}

pub(super) fn extract_signature(witness: &Witness) -> Option<Vec<u8>> {
    let invocation = witness.invocation_script();
    if invocation.len() != 66 {
        return None;
    }
    if invocation[0] != OpCode::PUSHDATA1.byte() || invocation[1] != 0x40 {
        return None;
    }
    Some(invocation[2..66].to_vec())
}

/// Builds the proposal policy service for one round: consensus limits from the
/// node settings plus the header/witness overhead of the active validator set.
pub(super) fn build_proposal_policy(
    system: &neo_system::NeoSystem,
    settings: &DbftSettings,
    validators: &[neo_crypto::ECPoint],
) -> ProposalPolicy {
    let protocol = system.settings();
    let limits = ProposalLimits::new(
        protocol.max_transactions_per_block as usize,
        settings.max_block_size as usize,
        settings.max_block_system_fee,
    );
    let required_signatures = neo_consensus::required_signatures(validators.len());
    ProposalPolicy::new(
        limits,
        estimate_header_size(validators, required_signatures),
    )
}

fn estimate_header_size(validators: &[neo_crypto::ECPoint], m: usize) -> usize {
    if validators.is_empty() || m == 0 {
        return 0;
    }

    let dummy_sig = vec![0u8; 64];
    let mut builder = ScriptBuilder::new();
    for _ in 0..m {
        builder.emit_push(&dummy_sig);
    }
    let invocation = builder.to_array();
    let verification = Contract::create_multi_sig_redeem_script(m, validators);
    let witness = Witness::new_with_scripts(invocation, verification);
    let mut header = Header::new();
    header.witness = witness;
    header.size()
}

pub(super) fn conflicts_with_context(
    tx: &Transaction,
    existing: &HashMap<UInt256, Transaction>,
) -> bool {
    let new_conflicts = conflict_hashes(tx);
    if new_conflicts.iter().any(|hash| existing.contains_key(hash)) {
        warn!(target: "neo", tx_hash = %tx.hash(), "transaction conflicts with existing set");
        return true;
    }

    for other in existing.values() {
        if conflict_hashes(other).iter().any(|hash| *hash == tx.hash()) {
            warn!(target: "neo", tx_hash = %tx.hash(), "existing transaction conflicts with new tx");
            return true;
        }
    }
    false
}

fn conflict_hashes(tx: &Transaction) -> Vec<UInt256> {
    tx.attributes()
        .iter()
        .filter_map(|attr| match attr {
            TransactionAttribute::Conflicts(conflicts) => Some(conflicts.hash),
            _ => None,
        })
        .collect()
}

pub(super) fn signer_accounts(tx: &Transaction) -> Vec<neo_primitives::UInt160> {
    tx.signers().iter().map(|signer| signer.account).collect()
}
