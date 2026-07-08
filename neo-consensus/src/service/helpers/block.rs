use crate::context::ValidatorInfo;
use neo_primitives::{UInt160, UInt256};

pub(in crate::service) struct ConsensusBlockFields;

impl ConsensusBlockFields {
    pub(in crate::service) fn compute_merkle_root(hashes: &[UInt256]) -> UInt256 {
        neo_crypto::MerkleTree::compute_root(hashes).unwrap_or_else(UInt256::zero)
    }

    /// The BFT threshold `M = N - (N-1)/3` for `n` validators (`0` for an
    /// empty set).
    pub(in crate::service) fn bft_threshold(n: usize) -> usize {
        neo_vm::script_builder::RedeemScript::bft_threshold(n)
    }

    /// The `M`-of-`N` multi-sig verification script over the validator public
    /// keys (C# `Contract.CreateMultiSigRedeemScript` with the canonically
    /// sorted keys): `PUSH(m) · {PUSH(key)}* · PUSH(n) · System.Crypto.CheckMultisig`.
    /// This is the block witness's verification script, and its hash is the
    /// `NextConsensus` address.
    pub(in crate::service) fn multisig_verification_script(
        keys: &[neo_crypto::ECPoint],
    ) -> Vec<u8> {
        use neo_vm::script_builder::ScriptBuilder;

        let n = keys.len();
        let m = ConsensusBlockFields::bft_threshold(n);

        let mut sorted = keys.to_vec();
        sorted.sort();

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);
        for key in &sorted {
            builder.emit_push(key.as_bytes());
        }
        builder.emit_push_int(n as i64);
        builder
            .emit_syscall("System.Crypto.CheckMultisig")
            .expect("infallible: in-memory emit");
        builder.to_array()
    }

    pub(in crate::service) fn compute_next_consensus_address(
        validators: &[ValidatorInfo],
    ) -> UInt160 {
        if validators.is_empty() {
            return UInt160::zero();
        }
        let keys: Vec<neo_crypto::ECPoint> =
            validators.iter().map(|v| v.public_key.clone()).collect();
        UInt160::from_script(&ConsensusBlockFields::multisig_verification_script(&keys))
    }

    // Rationale: header-hash construction is a protocol field list; grouping
    // into an ad-hoc struct would hide the serialized hash order.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::service) fn compute_header_hash(
        version: u32,
        prev_hash: UInt256,
        merkle_root: UInt256,
        timestamp: u64,
        nonce: u64,
        index: u32,
        primary_index: u8,
        next_consensus: UInt160,
    ) -> UInt256 {
        const HEADER_UNSIGNED_LEN: usize = std::mem::size_of::<u32>()
            + UInt256::LENGTH
            + UInt256::LENGTH
            + std::mem::size_of::<u64>()
            + std::mem::size_of::<u64>()
            + std::mem::size_of::<u32>()
            + std::mem::size_of::<u8>()
            + UInt160::LENGTH;

        let mut unsigned = Vec::with_capacity(HEADER_UNSIGNED_LEN);
        unsigned.extend_from_slice(&version.to_le_bytes());
        unsigned.extend_from_slice(&prev_hash.as_bytes());
        unsigned.extend_from_slice(&merkle_root.as_bytes());
        unsigned.extend_from_slice(&timestamp.to_le_bytes());
        unsigned.extend_from_slice(&nonce.to_le_bytes());
        unsigned.extend_from_slice(&index.to_le_bytes());
        unsigned.push(primary_index);
        unsigned.extend_from_slice(&next_consensus.as_bytes());

        // Matches C# `Verifiable.CalculateHash()` (single SHA-256 over unsigned bytes).
        UInt256::from(neo_crypto::Crypto::sha256(&unsigned))
    }
}
