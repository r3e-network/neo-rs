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

pub mod constants;
pub mod contract_parameter_type;
pub mod error;
pub mod hardfork;
pub mod inventory_type;
pub mod oracle_response_code;
pub mod transaction_attribute_type;
pub mod uint160;
pub mod uint256;
pub mod witness_scope;

// Re-exports
pub use constants::*;
pub use contract_parameter_type::ContractParameterType;
pub use error::{PrimitiveError, PrimitiveResult};
pub use hardfork::{Hardfork, HardforkParseError};
pub use inventory_type::InventoryType;
pub use oracle_response_code::OracleResponseCode;
pub use transaction_attribute_type::TransactionAttributeType;
pub use uint160::{UInt160, UINT160_SIZE};
pub use uint256::{UInt256, UINT256_SIZE};
pub use witness_scope::{InvalidWitnessScopeError, WitnessScope};
