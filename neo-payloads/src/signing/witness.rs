// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Witness - Transaction signature verification for Neo N3.
//!
//! This module provides the `Witness` struct, which contains the scripts needed
//! to verify transaction signatures on the Neo blockchain.
//!
//! ## Overview
//!
//! A witness consists of two parts:
//! - **Invocation Script**: Provides the arguments (signatures) for verification
//! - **Verification Script**: The contract code that verifies the signature
//!
//! ## Script Hash Computation
//!
//! The script hash is computed as: `RIPEMD160(SHA256(verification_script))`
//!
//! ## Example
//!
//! ```rust
//! use neo_payloads::Witness;
//!
//! // Create a witness from scripts
//! let invocation_script = vec![0x0C, 0x40]; // PUSHDATA1 64-byte
//! let verification_script = vec![0x0C, 0x21]; // PUSHDATA1 33-byte
//! let witness = Witness::new_with_scripts(
//!     invocation_script,
//!     verification_script,
//! );
//!
//! // Get the script hash
//! let script_hash = witness.script_hash();
//! ```

use base64::{Engine as _, engine::general_purpose};
use neo_crypto::Crypto;
use neo_io::{Serializable, serializable::helper::SerializeHelper};
use neo_primitives::UInt160;
use neo_primitives::hex_util;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;

/// Maximum size of invocation script in bytes.
/// This is designed to allow a MultiSig 21/11 (committee)
/// Invocation = 11 * (64 + 2) = 726
const MAX_INVOCATION_SCRIPT: usize = 1024;

/// Maximum size of verification script in bytes.
/// Verification = m + (PUSH_PubKey * 21) + length + null + syscall = 1 + ((2 + 33) * 21) + 2 + 1 + 5 = 744
const MAX_VERIFICATION_SCRIPT: usize = 1024;

/// Represents a witness of a verifiable object.
///
/// A witness contains the invocation script (used to pass arguments) and
/// the verification script (the contract code to verify the signature).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Witness {
    /// The invocation script of the witness. Used to pass arguments for verification script.
    pub invocation_script: Vec<u8>,

    /// The verification script of the witness. It can be empty if the contract is deployed.
    pub verification_script: Vec<u8>,

    /// Cached script hash
    #[serde(skip)]
    script_hash: OnceLock<UInt160>,
}

impl Witness {
    /// Creates a new Witness instance.
    pub fn new() -> Self {
        Self {
            invocation_script: Vec::new(),
            verification_script: Vec::new(),
            script_hash: OnceLock::new(),
        }
    }

    /// Creates a new Witness with the specified invocation and verification scripts.
    ///
    /// # Arguments
    ///
    /// * `invocation_script` - The invocation script
    /// * `verification_script` - The verification script
    ///
    /// # Returns
    ///
    /// A new Witness instance
    pub fn new_with_scripts(invocation_script: Vec<u8>, verification_script: Vec<u8>) -> Self {
        Self {
            invocation_script,
            verification_script,
            script_hash: OnceLock::new(),
        }
    }

    /// Creates an empty witness with empty invocation and verification scripts.
    ///
    /// # Returns
    ///
    /// An empty Witness instance
    pub fn empty() -> Self {
        Self::new()
    }

    /// Gets the hash of the verification script (matches C# ScriptHash property).
    /// Calculates RIPEMD160(SHA256(verification_script)) like the C# implementation.
    ///
    /// # Returns
    ///
    /// The script hash as UInt160
    pub fn script_hash(&self) -> UInt160 {
        *self
            .script_hash
            .get_or_init(|| UInt160::from(Crypto::hash160(&self.verification_script)))
    }

    /// Gets the size of the witness in bytes after serialization.
    ///
    /// # Returns
    ///
    /// The size in bytes
    pub fn size(&self) -> usize {
        SerializeHelper::get_var_size_bytes(&self.invocation_script)
            + SerializeHelper::get_var_size_bytes(&self.verification_script)
    }

    /// Converts the witness to JSON (matches C# `ToJson`).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "invocation": general_purpose::STANDARD.encode(&self.invocation_script),
            "verification": general_purpose::STANDARD.encode(&self.verification_script)
        })
    }

    /// Clones the witness.
    ///
    /// # Returns
    ///
    /// A cloned Witness instance
    pub fn clone_witness(&self) -> Self {
        Self {
            invocation_script: self.invocation_script.clone(),
            verification_script: self.verification_script.clone(),
            script_hash: {
                let clone_cell = OnceLock::new();
                if let Some(hash) = self.script_hash.get() {
                    let _ = clone_cell.set(*hash);
                }
                clone_cell
            },
        }
    }
}

neo_io::impl_default_via_new!(Witness);

impl neo_primitives::Witness for Witness {
    fn invocation_script(&self) -> &[u8] {
        &self.invocation_script
    }

    fn verification_script(&self) -> &[u8] {
        &self.verification_script
    }
}

impl Serializable for Witness {
    fn size(&self) -> usize {
        self.size()
    }

    fn serialize(&self, writer: &mut neo_io::BinaryWriter) -> neo_io::IoResult<()> {
        // Write invocation script with variable length encoding
        writer.write_var_bytes(&self.invocation_script)?;
        // Write verification script with variable length encoding
        writer.write_var_bytes(&self.verification_script)?;
        Ok(())
    }

    fn deserialize(reader: &mut neo_io::MemoryReader) -> neo_io::IoResult<Self> {
        // Read invocation script with variable length encoding
        let invocation_script = reader.read_var_bytes(MAX_INVOCATION_SCRIPT)?;

        // Read verification script with variable length encoding
        let verification_script = reader.read_var_bytes(MAX_VERIFICATION_SCRIPT)?;

        Ok(Self {
            invocation_script,
            verification_script,
            script_hash: OnceLock::new(),
        })
    }
}

impl fmt::Display for Witness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Witness {{ invocation: {}, verification: {} }}",
            hex_util::encode_hex(&self.invocation_script),
            hex_util::encode_hex(&self.verification_script)
        )
    }
}

#[cfg(test)]
#[path = "../tests/signing/witness.rs"]
mod tests;
