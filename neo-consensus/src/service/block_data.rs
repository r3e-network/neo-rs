use std::collections::HashMap;

use neo_crypto::ECPoint;
use neo_payloads::{Witness, block::Block, header::Header, transaction::Transaction};
use neo_primitives::{UInt160, UInt256};
use neo_vm::script_builder::ScriptBuilder;

use crate::error::{ConsensusError, ConsensusResult};
use crate::service::helpers::ConsensusBlockFields;

/// Block data for assembly by upper layers
#[derive(Debug, Clone)]
pub struct BlockData {
    /// Block index
    pub block_index: u32,
    /// Block timestamp
    pub timestamp: u64,
    /// Block nonce
    pub nonce: u64,
    /// Primary validator index
    pub primary_index: u8,
    /// Transaction hashes included in the block
    pub transaction_hashes: Vec<UInt256>,
    /// Commit signatures from validators (`validator_index`, signature)
    pub signatures: Vec<(u8, Vec<u8>)>,
    /// Validator public keys for multi-sig witness construction
    pub validator_pubkeys: Vec<ECPoint>,
    /// Required signature count (M in M-of-N multi-sig)
    pub required_signatures: usize,
    /// Header `NextConsensus` address agreed by the consensus round.
    pub next_consensus: UInt160,
}

impl BlockData {
    /// Assembles the final [`Block`] from the committed consensus data,
    /// mirroring C# `ConsensusContext.CreateBlock` / `EnsureHeader`:
    ///
    /// - `MerkleRoot = MerkleTree.ComputeRoot(TransactionHashes)`;
    /// - `NextConsensus` is the address carried by the committed consensus
    ///   context. At Neo committee-refresh heights this may differ from the
    ///   current validators that sign the block witness;
    /// - the block witness has that multi-sig as its verification script and
    ///   an invocation script that pushes the M commit signatures in the
    ///   verification script's canonical (sorted) key order, which is the
    ///   order C# `CheckMultisig` matches signatures against keys.
    ///
    /// `transactions` must be the full transactions for `transaction_hashes`
    /// (resolved by the caller from the mempool / ledger), in block order.
    pub fn assemble_block(
        &self,
        version: u32,
        prev_hash: UInt256,
        transactions: Vec<Transaction>,
    ) -> ConsensusResult<Block> {
        // Reuse the consensus crate's canonical computations so the assembled
        // block is byte-identical to the one the validators committed to:
        // MerkleRoot over the transaction hashes, and the M-of-N multi-sig
        // verification script over the current validators.
        let merkle_root = ConsensusBlockFields::compute_merkle_root(&self.transaction_hashes);
        let verification_script =
            ConsensusBlockFields::multisig_verification_script(&self.validator_pubkeys);

        // Push the commit signatures in the verification script's canonical
        // key order (C# `CheckMultisig` walks signatures and keys in lockstep,
        // so the invocation order must match the sorted key order).
        let signature_by_validator: HashMap<u8, &Vec<u8>> = self
            .signatures
            .iter()
            .map(|(index, sig)| (*index, sig))
            .collect();
        let mut sorted_validators: Vec<(usize, &ECPoint)> =
            self.validator_pubkeys.iter().enumerate().collect();
        sorted_validators.sort_by(|a, b| a.1.cmp(b.1));

        let mut builder = ScriptBuilder::new();
        let mut pushed = 0usize;
        for (validator_index, _key) in sorted_validators {
            if pushed >= self.required_signatures {
                break;
            }
            if let Some(signature) = signature_by_validator.get(&(validator_index as u8)) {
                builder.emit_push(signature);
                pushed += 1;
            }
        }
        if pushed < self.required_signatures {
            return Err(ConsensusError::InvalidProposal {
                message: format!(
                    "insufficient commit signatures: have {pushed}, need {}",
                    self.required_signatures
                ),
            });
        }

        let mut witness = Witness::new();
        witness.invocation_script = builder.to_array();
        witness.verification_script = verification_script;

        let header = Header::from_parts(
            version,
            prev_hash,
            merkle_root,
            self.timestamp,
            self.nonce,
            self.block_index,
            self.primary_index,
            self.next_consensus,
            witness,
        );
        Ok(Block::from_parts(header, transactions))
    }
}

#[cfg(test)]
#[path = "../tests/service/block_data.rs"]
mod tests;
