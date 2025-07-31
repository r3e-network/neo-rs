// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// modifications are permitted.

//! # Neo Core
//!
//! Core functionality for the Neo blockchain, including basic types and utilities.

// Module declarations
pub mod big_decimal;
pub mod block;
pub mod builders;
pub mod constants;
pub mod error;
pub mod error_utils;
pub mod events;
pub mod extensions;
pub mod hardfork;
pub mod neo_system;
pub mod shutdown;
pub mod signer;
pub mod transaction;
pub mod transaction_type;
pub mod uint160;
pub mod uint256;
pub mod witness;
pub mod witness_rule;
pub mod witness_scope;

// Re-exports
pub use big_decimal::BigDecimal;
pub use block::{Block, BlockHeader};
pub use error::{CoreError, CoreResult, Result};
pub use neo_system::NeoSystem;
pub use shutdown::{
    Shutdown, ShutdownCoordinator, ShutdownError, ShutdownEvent, ShutdownStage, SignalHandler,
};
pub use signer::Signer;
pub use transaction::{
    OracleResponseCode, Transaction, TransactionAttribute, TransactionAttributeType, HEADER_SIZE,
    MAX_TRANSACTION_ATTRIBUTES, MAX_TRANSACTION_SIZE,
};
pub use transaction_type::ContainsTransactionType;
pub use uint160::UInt160;
pub use uint256::UInt256;
pub use witness::Witness;
pub use witness_rule::{WitnessCondition, WitnessConditionType, WitnessRule, WitnessRuleAction};
pub use witness_scope::WitnessScope;

use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock};

/// Global blockchain singleton (matches C# Blockchain.Singleton exactly)
pub static GLOBAL_BLOCKCHAIN: Lazy<
    Arc<RwLock<Option<transaction::blockchain::BlockchainSingleton>>>,
> = Lazy::new(|| Arc::new(RwLock::new(None)));

/// Global store singleton (matches C# Store.Singleton exactly)
pub static GLOBAL_STORE: Lazy<
    Arc<RwLock<Option<Box<dyn transaction::blockchain::PersistenceStore + Send + Sync>>>>,
> = Lazy::new(|| Arc::new(RwLock::new(None)));

pub trait IVerifiable: std::any::Any {
    /// Verify the object
    fn verify(&self) -> bool;

    /// Get the hash of the object
    fn hash(&self) -> CoreResult<UInt256>;

    /// Get the hash data for signing
    fn get_hash_data(&self) -> Vec<u8>;

    /// Get as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}
