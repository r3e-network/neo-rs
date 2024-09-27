use std::fmt;
use std::cmp::min;

use crate::config;
use crate::core::dao;
use crate::core::interop;
use crate::core::native::nativenames;
use crate::core::transaction;
use crate::smartcontract;
use crate::smartcontract::callflag;
use crate::smartcontract::manifest;
use crate::util;
use crate::vm::stackitem;
use crate::vm::vmstate;

/// Ledger provides an interface to blocks/transactions storage for smart
/// contracts. It's not a part of the proper chain's state, so it's just a
/// proxy between regular Blockchain/DAO interface and smart contracts.
pub struct Ledger {
    contract_md: interop::ContractMD,
}

const LEDGER_CONTRACT_ID: i32 = -4;

impl Ledger {
    /// Creates a new Ledger native contract.
    pub fn new() -> Self {
        let mut l = Ledger {
            contract_md: interop::ContractMD::new(nativenames::LEDGER, LEDGER_CONTRACT_ID),
        };

        l.add_method("currentHash", Ledger::current_hash, smartcontract::Hash256Type, 1 << 15, callflag::READ_STATES);
        l.add_method("currentIndex", Ledger::current_index, smartcontract::IntegerType, 1 << 15, callflag::READ_STATES);
        l.add_method("getBlock", Ledger::get_block, smartcontract::ArrayType, 1 << 15, callflag::READ_STATES)
            .add_param("indexOrHash", smartcontract::ByteArrayType);
        l.add_method("getTransaction", Ledger::get_transaction, smartcontract::ArrayType, 1 << 15, callflag::READ_STATES)
            .add_param("hash", smartcontract::Hash256Type);
        l.add_method("getTransactionHeight", Ledger::get_transaction_height, smartcontract::IntegerType, 1 << 15, callflag::READ_STATES)
            .add_param("hash", smartcontract::Hash256Type);
        l.add_method("getTransactionFromBlock", Ledger::get_transaction_from_block, smartcontract::ArrayType, 1 << 16, callflag::READ_STATES)
            .add_param("blockIndexOrHash", smartcontract::ByteArrayType)
            .add_param("txIndex", smartcontract::IntegerType);
        l.add_method("getTransactionSigners", Ledger::get_transaction_signers, smartcontract::ArrayType, 1 << 15, callflag::READ_STATES)
            .add_param("hash", smartcontract::Hash256Type);
        l.add_method("getTransactionVMState", Ledger::get_transaction_vm_state, smartcontract::IntegerType, 1 << 15, callflag::READ_STATES)
            .add_param("hash", smartcontract::Hash256Type);

        l.build_hf_specific_md(l.active_in());
        l
    }

    /// Returns the contract metadata.
    pub fn metadata(&self) -> &interop::ContractMD {
        &self.contract_md
    }

    /// Initializes the contract.
    pub fn initialize(&self, _ic: &interop::Context, _hf: &config::Hardfork, _new_md: &interop::HFSpecificContractMD) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Initializes the contract cache.
    pub fn initialize_cache(&self, _block_height: u32, _d: &dao::Simple) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    /// Handles contract persistence.
    pub fn on_persist(&self, _ic: &interop::Context) -> Result<(), Box<dyn std::error::Error>> {
        // Actual block/tx processing is done in Blockchain.store_block().
        // Even though C# node add them to storage here, they're not
        // accessible to smart contracts (see is_traceable_block()), thus
        // the end effect is the same.
        Ok(())
    }

    /// Handles post-persistence operations.
    pub fn post_persist(&self, _ic: &interop::Context) -> Result<(), Box<dyn std::error::Error>> {
        Ok(()) // Actual block/tx processing is done in Blockchain.store_block().
    }

    /// Returns the hardfork in which this contract is active.
    pub fn active_in(&self) -> Option<config::Hardfork> {
        None
    }

    // Method implementations...
    // (current_hash, current_index, get_block, get_transaction, etc.)
    // These would be implemented similarly to the Go code, but adapted for Rust.

    // Helper functions...
    // (is_traceable_block, get_block_hash_from_item, get_uint256_from_item, get_transaction_and_height)
    // These would also be implemented similarly, but adapted for Rust.
}

// Additional helper functions and implementations as needed...
