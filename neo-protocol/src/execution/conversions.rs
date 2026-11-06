//! Existing payload/execution data conversions, now owned by the protocol.
//! Witness conversion constructs a fresh cache rather than propagating it.
use super::{Block as ExecutionBlock, BlockHeader};
use crate::payloads::{Block, Header};
use crate::witness::Witness;

/// Converts a payload witness to an execution witness with a fresh hash cache.
pub fn convert_payload_witness(witness: &Witness) -> Witness {
    Witness::new_with_scripts(
        witness.invocation_script().to_vec(),
        witness.verification_script().to_vec(),
    )
}
/// Converts a single-witness payload header to a one-element witness list.
pub fn convert_payload_header(header: &Header) -> BlockHeader {
    BlockHeader::new(
        header.version(),
        *header.prev_hash(),
        *header.merkle_root(),
        header.timestamp(),
        header.nonce(),
        header.index(),
        header.primary_index(),
        *header.next_consensus(),
        vec![convert_payload_witness(&header.witness)],
    )
}
/// Converts a payload block, cloning its transaction list as before.
pub fn convert_payload_block(block: &Block) -> ExecutionBlock {
    ExecutionBlock::new(
        convert_payload_header(&block.header),
        block.transactions.clone(),
    )
}
/// Converts an execution witness to a payload witness with a fresh hash cache.
pub fn convert_witness(witness: Witness) -> Witness {
    Witness::new_with_scripts(
        witness.invocation_script.clone(),
        witness.verification_script.clone(),
    )
}
/// Converts a list-witness header, selecting the first witness or an empty one.
pub fn convert_ledger_header(header: BlockHeader) -> Header {
    let mut converted = Header::new();
    converted.set_version(header.version);
    converted.set_prev_hash(header.previous_hash);
    converted.set_merkle_root(header.merkle_root);
    converted.set_timestamp(header.timestamp);
    converted.set_nonce(header.nonce);
    converted.set_index(header.index);
    converted.set_primary_index(header.primary_index);
    converted.set_next_consensus(header.next_consensus);
    let witness = header
        .witnesses
        .into_iter()
        .next()
        .unwrap_or_else(Witness::new);
    converted.witness = convert_witness(witness);
    converted
}
/// Converts an execution block, moving its transaction list as before.
pub fn convert_ledger_block(block: ExecutionBlock) -> Block {
    Block {
        header: convert_ledger_header(block.header),
        transactions: block.transactions,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt160;
    #[test]
    fn convert_witness_roundtrip() {
        let original = Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let back = convert_payload_witness(&convert_witness(original.clone()));
        assert_eq!(original.invocation_script, back.invocation_script);
        assert_eq!(original.verification_script, back.verification_script);
    }
    #[test]
    fn convert_header_preserves_fields() {
        let mut header = Header::new();
        header.set_version(1);
        header.set_index(100);
        header.set_timestamp(1234567890);
        header.set_nonce(42);
        header.set_primary_index(3);
        header.set_next_consensus(UInt160::zero());
        let converted = convert_payload_header(&header);
        assert_eq!(converted.version, 1);
        assert_eq!(converted.index, 100);
        assert_eq!(converted.timestamp, 1234567890);
        assert_eq!(converted.nonce, 42);
        assert_eq!(converted.primary_index, 3);
    }
    #[test]
    fn reverse_conversion_keeps_first_or_empty_witness() {
        let empty = convert_ledger_header(BlockHeader::default());
        assert!(empty.witness.invocation_script.is_empty());
        assert!(empty.witness.verification_script.is_empty());
        let first = Witness::new_with_scripts(vec![1], vec![2]);
        let mut header = BlockHeader::default();
        header.witnesses = vec![first.clone(), Witness::new_with_scripts(vec![3], vec![4])];
        let converted = convert_ledger_header(header);
        assert_eq!(converted.witness.invocation_script, first.invocation_script);
        assert_eq!(
            converted.witness.verification_script,
            first.verification_script
        );
        assert_eq!(convert_payload_header(&converted).witnesses.len(), 1);
    }
}
