//! # neo-primitives::protocol
//!
//! Protocol enums, versioned records, and chain-level domain constants.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `call_flags`: contract call-flag records.
//! - `contains_transaction_type`: transaction-container trait.
//! - `contract_basic_method`: contract basic method identifiers.
//! - `contract_parameter_type`: contract parameter type identifiers.
//! - `contract_task`: contract task records.
//! - `find_options`: storage find-option flags.
//! - `hardfork`: hardfork activation identifiers.
//! - `inventory_type`: P2P inventory-type identifiers.
//! - `log_level`: logging level identifiers.
//! - `node_capability_type`: P2P node capability identifiers.
//! - `oracle_response_code`: oracle response status codes.
//! - `transaction_attribute_type`: transaction attribute type types and
//!   helpers.
//! - `transaction_removal_reason`: transaction removal reason types and
//!   helpers.
//! - `trigger_type`: contract trigger type identifiers.
//! - `unhandled_exception_policy`: unhandled exception policy types and
//!   helpers.
//! - `verify_result`: verification result records.
//! - `witness_condition_type`: witness condition type identifiers.
//! - `witness_rule_action`: witness rule action identifiers.
//! - `witness_scope`: witness scope flags.

#[path = "execution/call_flags.rs"]
pub mod call_flags;
#[path = "ledger/contains_transaction_type.rs"]
pub mod contains_transaction_type;
#[path = "contracts/contract_basic_method.rs"]
pub mod contract_basic_method;
#[path = "contracts/contract_parameter_type.rs"]
pub mod contract_parameter_type;
#[path = "contracts/contract_task.rs"]
pub mod contract_task;
#[path = "storage/find_options.rs"]
pub mod find_options;
#[path = "chain/hardfork.rs"]
pub mod hardfork;
#[path = "network/inventory_type.rs"]
pub mod inventory_type;
/// Log-level primitives used by Neo diagnostics and extension utilities.
#[path = "diagnostics/log_level.rs"]
pub mod log_level;
#[path = "network/node_capability_type.rs"]
pub mod node_capability_type;
#[path = "oracle/oracle_response_code.rs"]
pub mod oracle_response_code;
#[path = "ledger/transaction_attribute_type.rs"]
pub mod transaction_attribute_type;
#[path = "ledger/transaction_removal_reason.rs"]
pub mod transaction_removal_reason;
#[path = "execution/trigger_type.rs"]
pub mod trigger_type;
#[path = "execution/unhandled_exception_policy.rs"]
pub mod unhandled_exception_policy;
#[path = "validation/verify_result.rs"]
pub mod verify_result;
#[path = "witness/witness_condition_type.rs"]
pub mod witness_condition_type;
#[path = "witness/witness_rule_action.rs"]
pub mod witness_rule_action;
#[path = "witness/witness_scope.rs"]
pub mod witness_scope;
