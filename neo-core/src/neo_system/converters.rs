//! Type conversion utilities between payload and ledger types.
//!
//! This module provides conversion functions between network payload types
//! (used in P2P communication) and ledger types (used in storage and processing).

use crate::ledger::{block::Block as LedgerBlock, block_header::BlockHeader as LedgerBlockHeader};
use crate::network::p2p::payloads::{
    block::Block, header::Header, witness::Witness as PayloadWitness,
};

/// Converts a payload witness to a ledger witness.
pub(crate) fn convert_payload_witness(witness: &PayloadWitness) -> crate::Witness {
    crate::Witness::new_with_scripts(
        witness.invocation_script().to_vec(),
        witness.verification_script().to_vec(),
    )
}

/// Converts a payload header to a ledger block header.
pub(crate) fn convert_payload_header(header: &Header) -> LedgerBlockHeader {
    LedgerBlockHeader::new(
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

/// Converts a payload block to a ledger block.
pub(crate) fn convert_payload_block(block: &Block) -> LedgerBlock {
    LedgerBlock::new(
        convert_payload_header(&block.header),
        block.transactions.clone(),
    )
}

/// Converts a ledger witness to a payload witness.
pub(crate) fn convert_witness(witness: crate::Witness) -> PayloadWitness {
    PayloadWitness::new_with_scripts(
        witness.invocation_script.clone(),
        witness.verification_script.clone(),
    )
}

/// Converts a ledger block header to a payload header.
pub(crate) fn convert_ledger_header(header: LedgerBlockHeader) -> Header {
    let LedgerBlockHeader {
        version,
        previous_hash,
        merkle_root,
        timestamp,
        nonce,
        index,
        primary_index,
        next_consensus,
        witnesses,
    } = header;

    let mut converted = Header::new();
    converted.set_version(version);
    converted.set_prev_hash(previous_hash);
    converted.set_merkle_root(merkle_root);
    converted.set_timestamp(timestamp);
    converted.set_nonce(nonce);
    converted.set_index(index);
    converted.set_primary_index(primary_index);
    converted.set_next_consensus(next_consensus);

    let witness = witnesses
        .into_iter()
        .next()
        .unwrap_or_else(crate::Witness::new);
    converted.witness = convert_witness(witness);

    converted
}

/// Converts a ledger block to a payload block.
pub(crate) fn convert_ledger_block(block: LedgerBlock) -> Block {
    Block {
        header: convert_ledger_header(block.header),
        transactions: block.transactions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UInt160;

    #[test]
    fn convert_witness_roundtrip() {
        let original = crate::Witness::new_with_scripts(vec![1, 2, 3], vec![4, 5, 6]);
        let payload = convert_witness(original.clone());
        let back = convert_payload_witness(&payload);
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

        let ledger_header = convert_payload_header(&header);
        assert_eq!(ledger_header.version, 1);
        assert_eq!(ledger_header.index, 100);
        assert_eq!(ledger_header.timestamp, 1234567890);
        assert_eq!(ledger_header.nonce, 42);
        assert_eq!(ledger_header.primary_index, 3);
    }
}
