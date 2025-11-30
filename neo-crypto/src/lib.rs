//! # Neo Crypto
//!
//! Cryptographic utilities for the Neo blockchain implementation.
//!
//! This crate provides cryptographic primitives required by Neo N3:
//!
//! ## Hash Functions
//! - **SHA-256**: Primary hash for transaction/block IDs
//! - **SHA-512**: Used in key derivation
//! - **RIPEMD-160**: Script hash computation (Hash160 = RIPEMD160(SHA256(data)))
//! - **Keccak-256**: Ethereum compatibility
//! - **Blake2b/Blake2s**: Alternative hash functions
//!
//! ## Elliptic Curve Cryptography
//! - **secp256r1 (P-256/NIST)**: Primary curve for Neo N3 signatures
//! - **secp256k1**: Bitcoin/Ethereum compatibility
//! - **Ed25519**: EdDSA signatures
//!
//! ## Design Principles
//!
//! - **Security**: All random number generation uses `OsRng` (cryptographically secure)
//! - **Compatibility**: Matches C# Neo implementation behavior
//! - **Performance**: Optimized for blockchain operations
//!
//! ## Example
//!
//! ```rust
//! use neo_crypto::{Crypto, HashAlgorithm};
//!
//! // Compute SHA-256 hash
//! let hash = Crypto::sha256(b"Hello, Neo!");
//!
//! // Compute Hash160 (RIPEMD160(SHA256(data)))
//! let script_hash = Crypto::hash160(b"contract script");
//! ```

pub mod error;
pub mod hash;
pub mod ecc;
pub mod named_curve_hash;

// Re-exports
pub use error::{CryptoError, CryptoResult};
pub use hash::{Crypto, HashAlgorithm};
pub use ecc::{ECPoint, ECCurve};
pub use named_curve_hash::NamedCurveHash;
