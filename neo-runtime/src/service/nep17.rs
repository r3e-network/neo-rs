//! NEP-17 token metadata reader trait.
//!
//! Defines the abstract boundary between the wallet layer (which needs
//! `symbol` / `decimals` to render transfer amounts) and the execution
//! layer (which runs a read-only contract call through `ApplicationEngine`
//! to obtain them).
//!
//! `neo-wallets` depends on this trait instead of `neo-execution` directly,
//! breaking the L4 → L3 execution-engine dependency for metadata reads.

use crate::error::ServiceError;
use neo_primitives::UInt160;
use std::fmt::Debug;

/// NEP-17 token metadata returned by [`Nep17MetadataReader::read_metadata`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nep17Metadata {
    /// The token's ticker symbol (e.g. `"GAS"`, `"NEO"`).
    pub symbol: String,
    /// The number of decimal places used by the token.
    pub decimals: u8,
}

/// Read-only NEP-17 token metadata provider.
///
/// Implemented by the execution layer; consumed by the wallet layer.
/// The concrete implementation runs a read-only `ApplicationEngine` script
/// that calls `symbol` and `decimals` on the target contract, matching
/// C# `Neo.Wallets.AssetDescriptor` semantics.
///
/// A single `read_metadata` call returns both fields so the implementor
/// can batch them into one VM execution (as the C# reference does).
pub trait Nep17MetadataReader: Send + Sync + Debug + 'static {
    /// Returns the symbol and decimals of the NEP-17 token at the given
    /// contract hash.
    ///
    /// Returns [`ServiceError::InvalidInput`] when the contract execution
    /// does not `HALT` or when the reported values are malformed.
    fn read_metadata(&self, contract_hash: UInt160) -> Result<Nep17Metadata, ServiceError>;
}
