//! Block data structures and validation logic.
//!
//! This module provides the core block data structures for the Neo blockchain,
//! exactly matching the C# Neo Block and Header structures.
//!
//! The implementation is split into modules that match the C# Neo structure:
//! - header: Block header structure and validation (matches Header.cs)
//! - block: Full block structure and validation (matches Block.cs)
//! - builder: Block builder for creating blocks (matches BlockBuilder pattern)
//! - verification: Cryptographic verification logic (matches verification methods)

pub mod block;
pub mod builder;
pub mod header;
pub mod verification;

// Re-export main types for convenience
pub use block::Block;
pub use builder::BlockBuilder;
pub use header::{BlockHeader, Header};

use crate::{Error, Result, VerifyResult};

/// Maximum number of transactions per block
pub const MAX_TRANSACTIONS_PER_BLOCK: usize = 512;

/// Maximum block size in bytes
pub const MAX_BLOCK_SIZE: usize = 262_144; // 256 KB

/// Helper trait to add script hash calculation to Vec<u8>
pub trait ScriptHashExt {
    fn to_script_hash(&self) -> neo_core::UInt160;
}

impl ScriptHashExt for Vec<u8> {
    fn to_script_hash(&self) -> neo_core::UInt160 {
        use ripemd::{Digest as RipemdDigest, Ripemd160};
        use sha2::{Digest, Sha256};

        // Hash160 = RIPEMD160(SHA256(script)) - matches C# exactly
        let mut sha256_hasher = Sha256::new();
        sha256_hasher.update(self);
        let sha256_result = sha256_hasher.finalize();

        let mut ripemd_hasher = Ripemd160::new();
        ripemd_hasher.update(&sha256_result);
        let ripemd_result = ripemd_hasher.finalize();

        neo_core::UInt160::from_bytes(&ripemd_result).unwrap_or_else(|_| neo_core::UInt160::zero())
    }
}
