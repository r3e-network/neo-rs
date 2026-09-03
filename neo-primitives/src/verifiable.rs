//! Verifiable trait for blockchain objects.
//!
//! This is the simplified base trait that lives in neo-primitives (Layer 0).
//! Neo-core extends it with additional methods that depend on DataCache,
//! ProtocolSettings, and the smart contract execution engine.

use crate::error::{PrimitiveError, PrimitiveResult};
use crate::UInt256;

/// Base trait for verifiable blockchain objects.
///
/// Provides hash computation and witness access without depending on
/// higher-layer types (DataCache, ProtocolSettings, ApplicationEngine).
///
/// # Implementors
///
/// - `Block` (neo-core)
/// - `Transaction` (neo-core)
/// - `Header` / `BlockHeader` (neo-core)
/// - `ExtensiblePayload` (neo-core)
pub trait Verifiable: std::any::Any + Send + Sync {
    /// Verifies the cryptographic validity of the object (state-independent checks only).
    fn verify(&self) -> bool;

    /// Computes the hash of the object.
    fn hash(&self) -> PrimitiveResult<UInt256>;

    /// Gets the serialized data used for hash computation (unsigned, no witnesses).
    fn hash_data(&self) -> Vec<u8>;

    /// Returns a reference to self as `Any` for downcasting.
    fn as_any(&self) -> &dyn std::any::Any;
}
