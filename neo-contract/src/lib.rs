//! # Neo Contract
//!
//! Smart contract execution engine for the Neo blockchain.
//!
//! This crate provides the contract execution layer that sits on top of neo-vm,
//! implementing Neo-specific contract semantics, native contracts, and interop services.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────┐
//! │           neo-contract                   │
//! │  ┌─────────────────────────────────────┐│
//! │  │      ApplicationEngine              ││
//! │  │  (Neo-specific VM wrapper)          ││
//! │  └─────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────┐│
//! │  │      Native Contracts               ││
//! │  │  (NEO, GAS, Policy, etc.)           ││
//! │  └─────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────┐│
//! │  │      Interop Services               ││
//! │  │  (System calls for contracts)       ││
//! │  └─────────────────────────────────────┘│
//! └─────────────────────────────────────────┘
//!                    │
//!                    ▼
//! ┌─────────────────────────────────────────┐
//! │              neo-vm                      │
//! │  (Stack-based bytecode execution)       │
//! └─────────────────────────────────────────┘
//! ```
//!
//! ## Components
//!
//! - **ApplicationEngine**: High-level VM wrapper with Neo-specific features
//! - **Native Contracts**: Built-in contracts (NEO, GAS, Policy, Oracle, etc.)
//! - **Manifest**: Contract metadata and permissions
//! - **Interop Services**: System calls available to contracts
//!
//! ## Core Types
//!
//! - [`TriggerType`]: Specifies when a contract is triggered (Application, Verification, etc.)
//! - [`ContractParameterType`]: Parameter types for contract methods
//! - [`FindOptions`]: Options for storage iteration
//! - [`CallFlags`]: Re-exported from neo-vm for contract call permissions
//!
//! ## Example
//!
//! ```rust
//! use neo_contract::{TriggerType, ContractParameterType, FindOptions};
//!
//! // Check trigger type
//! let trigger = TriggerType::APPLICATION;
//! assert!(trigger.contains(TriggerType::APPLICATION));
//!
//! // Parameter types for contract ABI
//! let param_type = ContractParameterType::Hash160;
//! assert_eq!(param_type.as_str(), "Hash160");
//!
//! // Storage find options
//! let opts = FindOptions::KEYS_ONLY | FindOptions::BACKWARDS;
//! assert!(opts.contains(FindOptions::KEYS_ONLY));
//! ```

pub mod contract_basic_method;
pub mod contract_parameter_type;
pub mod error;
pub mod find_options;
pub mod method_token;
pub mod role;
pub mod storage_context;
pub mod trigger_type;

// Re-exports
pub use contract_basic_method::ContractBasicMethod;
pub use contract_parameter_type::ContractParameterType;
pub use error::{ContractError, ContractResult};
pub use find_options::FindOptions;
pub use method_token::{MethodToken, MethodTokenError, MAX_METHOD_NAME_LENGTH};
pub use role::Role;
pub use storage_context::{StorageContext, StorageContextError};
pub use trigger_type::TriggerType;

// Re-export CallFlags from neo-vm for convenience
pub use neo_vm::call_flags::CallFlags;

// Placeholder for future modules
// pub mod application_engine;
// pub mod native;
// pub mod manifest;
// pub mod interop;
