//! # neo-primitives
//!
//! Foundational hashes, integers, addresses, and protocol primitive types.
//!
//! ## Boundary
//!
//! This foundation crate must stay free of node-service, storage-backend, RPC,
//! and network orchestration dependencies.
//!
//! ## Contents
//!
//! - `errors`: Typed errors and result aliases for this crate boundary.
//! - `numeric`: Fixed-size numeric wrappers and byte-order conversion helpers.
//! - `payload`: Payload-domain primitives shared by protocol and network
//!   crates.
//! - `protocol`: Protocol enums, versioned records, and chain-level domain
//!   constants.
//! - `utils`: Small utility helpers shared within the crate.
//! - `blockchain`: Blockchain-domain primitive records used across crates.
//! - `macros`: Crate-local macros that keep protocol declarations compact.
//! - `tests`: Module-local tests and regression coverage.

#[doc(hidden)]
pub use bitflags;

mod errors;
mod numeric;
mod payload;
mod protocol;
mod utils;

pub use errors::{error, network_error, rpc_exception};
pub(crate) use numeric::uint_hex;
pub use numeric::{base58_check, big_decimal, hex_util, uint160, uint256};
pub use payload::{inventory, serializable_payload, storage, verifiable};
pub use protocol::{
    call_flags, contains_transaction_type, contract_basic_method, contract_parameter_type,
    contract_task, find_options, hardfork, inventory_type, log_level, node_capability_type,
    oracle_response_code, transaction_attribute_type, transaction_removal_reason, trigger_type,
    unhandled_exception_policy, verify_result, witness_condition_type, witness_rule_action,
    witness_scope,
};
pub use utils::{constants, time};

pub mod blockchain;
/// Macro helpers for compact protocol enum declarations.
#[path = "macros/mod.rs"]
pub mod macros;

/// Re-export of the canonical hex prefix stripper (ADR-024).
///
/// The legacy re-export from `uint_hex` is kept for backward compatibility —
/// `uint_hex::strip_hex_prefix` now delegates to `hex_util::strip_hex_prefix`.
pub use hex_util::strip_hex_prefix;

pub use big_decimal::BigDecimal;
pub use witness_rule_action::WitnessRuleAction;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub use tests::*;

// Re-exports
pub use call_flags::CallFlags;
pub use constants::*;
pub use contains_transaction_type::ContainsTransactionType;
pub use contract_basic_method::ContractBasicMethod;
pub use contract_parameter_type::ContractParameterType;
pub use contract_task::ContractTask;
pub use error::{PrimitiveError, PrimitiveResult};
pub use find_options::FindOptions;
pub use hardfork::{Hardfork, HardforkParseError};
pub use inventory::Inventory;
pub use inventory_type::InventoryType;
pub use log_level::LogLevel;
pub use network_error::{NetworkError, NetworkResult};
pub use node_capability_type::NodeCapabilityType;
pub use oracle_response_code::OracleResponseCode;
pub use rpc_exception::RpcException;
pub use transaction_attribute_type::TransactionAttributeType;
pub use transaction_removal_reason::TransactionRemovalReason;
pub use trigger_type::TriggerType;
pub use uint160::{UINT160_SIZE, UInt160};
pub use uint256::{UINT256_SIZE, UInt256};
pub use verifiable::Verifiable;
pub use verify_result::VerifyResult;
pub use witness_condition_type::WitnessConditionType;
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};

// Marker traits used to decouple higher-level crates from concrete chain types.
pub use blockchain::BlockLike;
pub use serializable_payload::SerializablePayload;
pub use storage::{StorageValue, StorageValueError, StorageValueResult};
pub use time::{TimeProvider, TimeSource};
pub use unhandled_exception_policy::{UnhandledExceptionPolicy, panic_message};
