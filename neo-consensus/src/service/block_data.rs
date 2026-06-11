use std::collections::HashMap;

use neo_crypto::ECPoint;
use neo_payloads::{block::Block, header::Header, transaction::Transaction, Witness};
use neo_primitives::{UInt160, UInt256};
use neo_script_builder::ScriptBuilder;

use crate::error::{ConsensusError, ConsensusResult};
use crate::service::helpers::{compute_merkle_root, multisig_verification_script};

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
}

impl BlockData {
    /// Assembles the final [`Block`] from the committed consensus data,
    /// mirroring C# `ConsensusContext.CreateBlock` / `EnsureHeader`:
    ///
    /// - `MerkleRoot = MerkleTree.ComputeRoot(TransactionHashes)`;
    /// - `NextConsensus = Contract.GetBFTAddress(validators)` — the script
    ///   hash of the M-of-N multi-sig over the validator public keys (for a
    ///   static committee the next validators equal the current ones);
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
        // verification script / `NextConsensus` over the validators.
        let merkle_root = compute_merkle_root(&self.transaction_hashes);
        let verification_script = multisig_verification_script(&self.validator_pubkeys);
        let next_consensus = UInt160::from_script(&verification_script);

        // Push the commit signatures in the verification script's canonical
        // key order (C# `CheckMultisig` walks signatures and keys in lockstep,
        // so the invocation order must match the sorted key order).
        let signature_by_validator: HashMap<u8, &Vec<u8>> =
            self.signatures.iter().map(|(index, sig)| (*index, sig)).collect();
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
            next_consensus,
            witness,
        );
        Ok(Block::from_parts(header, transactions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::Secp256r1Crypto;

    fn test_pubkey(byte: u8) -> ECPoint {
        let mut private_key = [0u8; 32];
        private_key[31] = byte.max(1);
        let public = Secp256r1Crypto::derive_public_key(&private_key).expect("derive key");
        ECPoint::from_bytes(&public).expect("decode point")
    }

    /// A single-validator (1-of-1) commit assembles into a block whose header
    /// carries the correct Merkle root, `NextConsensus` (= the 1-of-1 multi-sig
    /// script hash), and a witness pairing that multi-sig with the pushed
    /// commit signature.
    #[test]
    fn assemble_block_builds_header_and_multisig_witness() {
        let pubkey = test_pubkey(1);
        let signature = vec![0xABu8; 64];
        let data = BlockData {
            block_index: 1,
            timestamp: 12_345,
            nonce: 7,
            primary_index: 0,
            transaction_hashes: Vec::new(),
            signatures: vec![(0u8, signature.clone())],
            validator_pubkeys: vec![pubkey.clone()],
            required_signatures: 1,
        };

        let block = data
            .assemble_block(0, UInt256::zero(), Vec::new())
            .expect("assemble");

        // Header fields carried through from the committed consensus data.
        assert_eq!(block.header.index(), 1);
        assert_eq!(block.header.primary_index(), 0);
        assert_eq!(block.header.timestamp(), 12_345);

        // Merkle root of an empty block is the zero hash.
        assert_eq!(*block.header.merkle_root(), UInt256::zero());

        // NextConsensus == hash of the 1-of-1 multi-sig over the validator key,
        // and that script is the witness verification script.
        let expected_script = multisig_verification_script(&[pubkey]);
        assert_eq!(*block.header.next_consensus(), UInt160::from_script(&expected_script));
        assert_eq!(block.header.witness.verification_script, expected_script);

        // The invocation script pushes the single commit signature.
        assert!(
            block.header.witness.invocation_script.windows(signature.len()).any(|w| w == signature),
            "invocation script must contain the pushed commit signature"
        );
    }

    /// Assembly fails cleanly when fewer commit signatures than required are
    /// present (rather than producing an unverifiable witness).
    #[test]
    fn assemble_block_rejects_insufficient_signatures() {
        let data = BlockData {
            block_index: 2,
            timestamp: 1,
            nonce: 0,
            primary_index: 0,
            transaction_hashes: Vec::new(),
            signatures: Vec::new(),
            validator_pubkeys: vec![test_pubkey(1), test_pubkey(2), test_pubkey(3)],
            required_signatures: 2,
        };
        assert!(data.assemble_block(0, UInt256::zero(), Vec::new()).is_err());
    }
}
