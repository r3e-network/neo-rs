//! Genesis block creation utilities.
//!
//! Neo N3 defines a fixed, deterministic genesis block derived from protocol settings:
//! - No transactions
//! - Merkle root set to zero
//! - Timestamp/nonce are fixed constants
//! - NextConsensus is derived from the standby validators via the BFT multisig address
//!
//! Migrated from `neo-core/src/ledger/genesis.rs`. The returned value is the
//! canonical network payload block ([`neo_protocol::payloads::Block`]) consumed by
//! the decomposed execution engine, rather than neo-core's ledger `Block`.
//!
//! Import adaptation:
//! - `crate::Witness` → `neo_protocol::payloads::Witness`
//! - `crate::constants::GENESIS_TIMESTAMP_MS` → `neo_primitives::constants::GENESIS_TIMESTAMP_MS`
//! - `crate::ledger::{Block, BlockHeader}` → `neo_protocol::payloads::{Block, Header}`
//! - `crate::protocol_settings::ProtocolSettings` → `neo_protocol::ProtocolSettings`
//! - `crate::smart_contract::native::helpers::NativeHelpers` → `crate::native::helpers::NativeHelpers`
//! - `crate::wallets::helper::Helper` (tests) → `neo_wallets::helper::Helper`

use crate::native::helpers::NativeHelpers;
use neo_primitives::constants::GENESIS_TIMESTAMP_MS;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::{Block, Header, Witness};
use neo_vm::OpCode;

/// The fixed consensus nonce of the Neo N3 genesis block (C# `GenesisBlock.Nonce`).
const GENESIS_NONCE: u64 = 2_083_236_893;

/// Creates the genesis block for the given protocol settings.
pub fn create_genesis_block(settings: &ProtocolSettings) -> Block {
    let validators = settings.standby_validators();
    let next_consensus = if validators.is_empty() {
        UInt160::zero()
    } else {
        NativeHelpers::get_bft_address(&validators)
    };

    let mut header = Header::new();
    header.set_version(0);
    header.set_prev_hash(UInt256::zero());
    header.set_merkle_root(UInt256::zero());
    header.set_timestamp(GENESIS_TIMESTAMP_MS);
    header.set_nonce(GENESIS_NONCE);
    header.set_index(0);
    header.set_primary_index(0);
    header.set_next_consensus(next_consensus);
    // Genesis carries a single trivially-true witness: an empty invocation
    // script against a `PUSH1` verification script.
    header.witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);

    Block {
        header,
        transactions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_wallets::helper::Helper;

    #[test]
    fn genesis_block_has_correct_index() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.index(), 0);
    }

    #[test]
    fn genesis_block_has_zero_prev_hash() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert_eq!(genesis.prev_hash(), &UInt256::zero());
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
        assert_eq!(genesis.timestamp(), GENESIS_TIMESTAMP_MS);
    }

    #[test]
    fn genesis_block_witness_is_push1_verification_script() {
        let settings = ProtocolSettings::default_settings();
        let genesis = create_genesis_block(&settings);
        assert!(genesis.witness().invocation_script().is_empty());
        assert_eq!(
            genesis.witness().verification_script(),
            &[OpCode::PUSH1.byte()][..]
        );
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
        assert_eq!(genesis.next_consensus(), &expected);
    }

    #[test]
    fn mainnet_genesis_hash_matches_csharp() {
        let settings = ProtocolSettings::mainnet();
        let mut genesis = create_genesis_block(&settings);
        let expected =
            UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
                .expect("reference genesis hash should parse");
        assert_eq!(genesis.hash(), expected);
    }
}
