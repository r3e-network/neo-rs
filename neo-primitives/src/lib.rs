#![warn(missing_docs)]
//! # Neo Primitives
//!
//! Fundamental types for the Neo blockchain implementation.
//!
//! This crate provides the core primitive types used throughout the Neo ecosystem:
//! - `UInt160`: 160-bit unsigned integer (script hashes, addresses)
//! - `UInt256`: 256-bit unsigned integer (transaction/block hashes)
//! - `BigDecimal`: Arbitrary precision decimal for financial calculations
//!
//! ## Design Principles
//!
//! - **Zero dependencies on other neo-* crates** (except neo-io for serialization traits)
//! - **C# Neo compatibility**: Matches the behavior of Neo C# implementation
//! - **Efficient**: Optimized for blockchain operations
//!
//! ## Example
//!
//! ```rust
//! use neo_primitives::{UInt160, UInt256};
//!
//! // Create from bytes
//! let hash = UInt256::zero();
//! assert!(hash.is_zero());
//!
//! // Parse from hex string
//! let address_hash = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();
//! ```

#[doc(hidden)]
pub use bitflags;

pub mod base58_check;
pub mod big_decimal;
pub mod blockchain;
pub mod call_flags;
pub mod constants;
pub mod contains_transaction_type;
pub mod contract_basic_method;
pub mod contract_task;
pub mod find_options;
pub mod contract_parameter_type;
pub mod error;
pub mod hardfork;
pub mod inventory;
pub mod inventory_type;
pub mod log_event_args;
pub mod log_level;
/// Macro helpers for compact protocol enum declarations.
pub mod macros;
pub mod network_error;
pub mod node_capability_type;
pub mod oracle_response_code;
pub mod rpc_exception;
pub mod storage;
pub mod transaction_attribute_type;
pub mod transaction_removal_reason;
pub mod trigger_type;
pub mod verifiable;
pub mod uint160;
pub mod uint256;
mod uint_hex;
pub mod serializable_payload;
pub mod unhandled_exception_policy;
pub mod verification;
pub mod verify_result;
pub mod witness_condition_type;
pub mod witness_rule_action;
pub mod witness_scope;

pub use big_decimal::BigDecimal;
pub use witness_rule_action::WitnessRuleAction;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use tests::*;

// Re-exports
pub use constants::*;
pub use call_flags::CallFlags;
pub use contains_transaction_type::ContainsTransactionType;
pub use contract_basic_method::ContractBasicMethod;
pub use contract_task::ContractTask;
pub use find_options::FindOptions;
pub use contract_parameter_type::ContractParameterType;
pub use error::{PrimitiveError, PrimitiveResult};
pub use hardfork::{Hardfork, HardforkParseError};
pub use inventory::Inventory;
pub use inventory_type::InventoryType;
pub use log_event_args::LogEventArgs;
pub use log_level::LogLevel;
pub use network_error::{NetworkError, NetworkResult};
pub use node_capability_type::NodeCapabilityType;
pub use oracle_response_code::OracleResponseCode;
pub use rpc_exception::RpcException;
pub use transaction_attribute_type::TransactionAttributeType;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use trigger_type::TriggerType;
pub use verifiable::Verifiable;
pub use uint160::{UInt160, UINT160_SIZE};
pub use uint256::{UInt256, UINT256_SIZE};
pub use verify_result::VerifyResult;
pub use witness_condition_type::WitnessConditionType;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};

// Marker traits used to decouple higher-level crates from concrete chain types.
pub use blockchain::{BlockLike, NetworkMessage};
pub use storage::{StorageValue, StorageValueError, StorageValueResult};
pub use serializable_payload::SerializablePayload;
pub use unhandled_exception_policy::{panic_message, UnhandledExceptionPolicy};
pub use verification::{
    BlockchainSnapshot, VerificationContext, Witness, VerificationError, VerificationResult,
};


/// Implements `Default` by returning `Self::new()`.
///
/// Mirrors the C# `Neo.IO.Helper` `impl_default_via_new!` macro that
/// the original `neo-core` used to derive `Default` for value types.
#[macro_export]
macro_rules! impl_default_via_new {
    ($name:ident) => {
        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}
