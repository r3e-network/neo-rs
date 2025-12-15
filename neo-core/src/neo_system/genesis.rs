//! Genesis block creation utilities.
//!
//! This module provides functions for creating the genesis block
//! based on protocol settings.

use crate::constants::GENESIS_TIMESTAMP_MS;
use crate::network::p2p::payloads::{
    block::Block, header::Header, witness::Witness as PayloadWitness,
};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::helpers::NativeHelpers;
use neo_primitives::{UInt160, UInt256};
use neo_vm::OpCode;

/// Creates the genesis block for the given protocol settings.
///
/// The genesis block is the first block in the blockchain and contains
/// no transactions. Its structure is determined by the protocol settings,
/// particularly the standby validators which determine the next consensus address.
///
/// # Arguments
///
/// * `settings` - The protocol settings to use for genesis block creation
///
/// # Returns
///
/// A new `Block` representing the genesis block
pub fn create_genesis_block(settings: &ProtocolSettings) -> Block {
    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_timestamp(GENESIS_TIMESTAMP_MS);
    header.set_nonce(2_083_236_893u64);
    header.set_index(0);
    header.set_primary_index(0);

    let validators = settings.standby_validators();
    let next_consensus = if validators.is_empty() {
        UInt160::zero()
    } else {
        NativeHelpers::get_bft_address(&validators)
    };
    header.set_next_consensus(next_consensus);
    header.witness = PayloadWitness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1 as u8]);

    Block {
        header,
        transactions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genesis_block_has_correct_index() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.header.index(), 0);
    }

    #[test]
    fn genesis_block_has_zero_prev_hash() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.header.prev_hash(), &UInt256::zero());
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
        assert_eq!(genesis.header.timestamp(), GENESIS_TIMESTAMP_MS);
    }
}
