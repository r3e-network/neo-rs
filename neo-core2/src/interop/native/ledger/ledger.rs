/// Package ledger provides an interface to LedgerContract native contract.
/// It allows to access ledger contents like transactions and blocks.

pub mod ledger {
    use crate::interop::{self, contract, neogointernal};

    // Hash represents Ledger contract hash.
    pub const HASH: &str = "\xbe\xf2\x04\x31\x40\x36\x2a\x77\xc1\x50\x99\xc7\xe6\x4c\x12\xf7\x00\xb6\x65\xda";

    // VMState represents VM execution state.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum VMState {
        NoneState = 0,
        HaltState = 1,
        FaultState = 2,
        BreakState = 4,
    }

    // CurrentHash represents `currentHash` method of Ledger native contract.
    pub fn current_hash() -> interop::Hash256 {
        neogointernal::call_with_token(HASH, "currentHash", contract::ReadStates as i32)
    }

    // CurrentIndex represents `currentIndex` method of Ledger native contract.
    pub fn current_index() -> i32 {
        neogointernal::call_with_token(HASH, "currentIndex", contract::ReadStates as i32)
    }

    // GetBlock represents `getBlock` method of Ledger native contract.
    pub fn get_block(index_or_hash: impl Into<interop::Any>) -> Option<interop::Block> {
        neogointernal::call_with_token(HASH, "getBlock", contract::ReadStates as i32, index_or_hash.into())
    }

    // GetTransaction represents `getTransaction` method of Ledger native contract.
    pub fn get_transaction(hash: interop::Hash256) -> Option<interop::Transaction> {
        neogointernal::call_with_token(HASH, "getTransaction", contract::ReadStates as i32, hash)
    }

    // GetTransactionHeight represents `getTransactionHeight` method of Ledger native contract.
    pub fn get_transaction_height(hash: interop::Hash256) -> i32 {
        neogointernal::call_with_token(HASH, "getTransactionHeight", contract::ReadStates as i32, hash)
    }

    // GetTransactionFromBlock represents `getTransactionFromBlock` method of Ledger native contract.
    pub fn get_transaction_from_block(index_or_hash: impl Into<interop::Any>, tx_index: i32) -> Option<interop::Transaction> {
        neogointernal::call_with_token(HASH, "getTransactionFromBlock", contract::ReadStates as i32, index_or_hash.into(), tx_index)
    }

    // GetTransactionSigners represents `getTransactionSigners` method of Ledger native contract.
    pub fn get_transaction_signers(hash: interop::Hash256) -> Vec<interop::TransactionSigner> {
        neogointernal::call_with_token(HASH, "getTransactionSigners", contract::ReadStates as i32, hash)
    }

    // GetTransactionVMState represents `getTransactionVMState` method of Ledger native contract.
    pub fn get_transaction_vm_state(hash: interop::Hash256) -> VMState {
        neogointernal::call_with_token(HASH, "getTransactionVMState", contract::ReadStates as i32, hash)
    }
}
