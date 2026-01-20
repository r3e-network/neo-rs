//! Genesis block creation utilities.
//!
//! Neo N3 defines a fixed, deterministic genesis block derived from protocol settings:
//! - No transactions
//! - Merkle root set to zero
//! - Timestamp/nonce are fixed constants
//! - NextConsensus is derived from the standby validators via the BFT multisig address

use crate::constants::GENESIS_TIMESTAMP_MS;
use crate::ledger::{Block, BlockHeader};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::Witness;
use neo_primitives::{UInt160, UInt256};
use neo_vm::OpCode;

/// Creates the genesis block for the given protocol settings.
pub fn create_genesis_block(settings: &ProtocolSettings) -> Block {
    let validators = settings.standby_validators();
    let next_consensus = if validators.is_empty() {
        UInt160::zero()
    } else {
        NativeHelpers::get_bft_address(&validators)
    };
    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        GENESIS_TIMESTAMP_MS,
        2_083_236_893u64,
        0,
        0,
        next_consensus,
        vec![Witness::new_with_scripts(
            Vec::new(),
            vec![OpCode::PUSH1 as u8],
        )],
    );

    Block::new(header, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallets::helper::Helper;

    #[test]
    fn genesis_block_has_correct_index() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.header.index, 0);
    }

    #[test]
    fn genesis_block_has_zero_prev_hash() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.header.previous_hash, UInt256::zero());
    }

    #[test]
    fn genesis_block_has_no_transactions() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert!(genesis.transactions.is_empty());
    }

    #[test]
    fn genesis_block_has_correct_timestamp() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.header.timestamp, GENESIS_TIMESTAMP_MS);
    }

    #[test]
    fn mainnet_genesis_next_consensus_matches_csharp() {
        let settings = ProtocolSettings::mainnet();
        let genesis = create_genesis_block(&settings);
        let expected = Helper::to_script_hash(
            "NVg7LjGcUSrgxgjX3zEgqaksfMaiS8Z6e1",
            settings.address_version,
        )
        .expect("reference address should decode");
        assert_eq!(genesis.header.next_consensus, expected);
    }

    #[test]
    fn mainnet_genesis_hash_matches_csharp() {
        let settings = ProtocolSettings::mainnet();
        let genesis = create_genesis_block(&settings);
        let expected =
            UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
                .expect("reference genesis hash should parse");
        assert_eq!(genesis.hash(), expected);
    }
}
