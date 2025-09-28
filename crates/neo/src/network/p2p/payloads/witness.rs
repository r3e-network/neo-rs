// Copyright (C) 2015-2025 The Neo Project.
//
// witness.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::neo_crypto::{ripemd160, sha256};
use crate::neo_io::{MemoryReader, Serializable};
use crate::UInt160;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::sync::Mutex;

// This is designed to allow a MultiSig 21/11 (committee)
// Invocation = 11 * (64 + 2) = 726
const MAX_INVOCATION_SCRIPT: usize = 1024;

// Verification = m + (PUSH_PubKey * 21) + length + null + syscall = 1 + ((2 + 33) * 21) + 2 + 1 + 5 = 744
const MAX_VERIFICATION_SCRIPT: usize = 1024;

/// Represents a witness of an IVerifiable object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Witness {
    /// The invocation script of the witness. Used to pass arguments for verification_script.
    pub invocation_script: Vec<u8>,

    /// The verification script of the witness. It can be empty if the contract is deployed.
    pub verification_script: Vec<u8>,

    #[serde(skip)]
    _script_hash: Mutex<Option<UInt160>>,
}

impl Witness {
    /// Creates a new empty witness.
    pub fn new() -> Self {
        Self {
            invocation_script: Vec::new(),
            verification_script: Vec::new(),
            _script_hash: Mutex::new(None),
        }
    }

    /// Creates a new witness with the given scripts.
    pub fn new_with_scripts(invocation_script: Vec<u8>, verification_script: Vec<u8>) -> Self {
        Self {
            invocation_script,
            verification_script,
            _script_hash: Mutex::new(None),
        }
    }

    /// Gets the hash of the verification script.
    /// Matches C# ScriptHash property exactly.
    pub fn script_hash(&self) -> UInt160 {
        let mut hash_guard = self._script_hash.lock().unwrap();
        if let Some(hash) = *hash_guard {
            return hash;
        }

        // Calculate script hash from verification script
        // This matches C# ToScriptHash() extension method
        let sha = sha256(&self.verification_script);
        let hash = UInt160::from(ripemd160(&sha));
        *hash_guard = Some(hash);
        hash
    }

    /// Creates an empty witness.
    pub fn empty() -> Self {
        Self::new()
    }

    /// Converts the witness to JSON.
    /// Matches C# ToJson() exactly.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "invocation": base64::encode(&self.invocation_script),
            "verification": base64::encode(&self.verification_script)
        })
    }

    /// Clones the witness.
    /// Matches C# Clone() exactly.
    pub fn clone_witness(&self) -> Self {
        let hash = self._script_hash.lock().unwrap().clone();
        Self {
            invocation_script: self.invocation_script.clone(),
            verification_script: self.verification_script.clone(),
            _script_hash: Mutex::new(hash),
        }
    }
}

impl Default for Witness {
    fn default() -> Self {
        Self::new()
    }
}

impl Serializable for Witness {
    fn size(&self) -> usize {
        // Matches C# Size property: InvocationScript.GetVarSize() + VerificationScript.GetVarSize()
        let invocation_var_size = if self.invocation_script.len() < 0xFD {
            1 + self.invocation_script.len()
        } else if self.invocation_script.len() <= 0xFFFF {
            3 + self.invocation_script.len()
        } else {
            5 + self.invocation_script.len()
        };

        let verification_var_size = if self.verification_script.len() < 0xFD {
            1 + self.verification_script.len()
        } else if self.verification_script.len() <= 0xFFFF {
            3 + self.verification_script.len()
        } else {
            5 + self.verification_script.len()
        };

        invocation_var_size + verification_var_size
    }

    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        // Write invocation script as var bytes
        if self.invocation_script.len() > MAX_INVOCATION_SCRIPT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invocation script too long",
            ));
        }

        // Write var-length encoded size
        if self.invocation_script.len() < 0xFD {
            writer.write_all(&[self.invocation_script.len() as u8])?;
        } else if self.invocation_script.len() <= 0xFFFF {
            writer.write_all(&[0xFD])?;
            writer.write_all(&(self.invocation_script.len() as u16).to_le_bytes())?;
        } else {
            writer.write_all(&[0xFE])?;
            writer.write_all(&(self.invocation_script.len() as u32).to_le_bytes())?;
        }
        writer.write_all(&self.invocation_script)?;

        // Write verification script as var bytes
        if self.verification_script.len() > MAX_VERIFICATION_SCRIPT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Verification script too long",
            ));
        }

        // Write var-length encoded size
        if self.verification_script.len() < 0xFD {
            writer.write_all(&[self.verification_script.len() as u8])?;
        } else if self.verification_script.len() <= 0xFFFF {
            writer.write_all(&[0xFD])?;
            writer.write_all(&(self.verification_script.len() as u16).to_le_bytes())?;
        } else {
            writer.write_all(&[0xFE])?;
            writer.write_all(&(self.verification_script.len() as u32).to_le_bytes())?;
        }
        writer.write_all(&self.verification_script)?;

        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, String> {
        let invocation_script = reader
            .read_var_bytes_max(MAX_INVOCATION_SCRIPT)
            .map_err(|e| e.to_string())?;
        let verification_script = reader
            .read_var_bytes_max(MAX_VERIFICATION_SCRIPT)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            invocation_script,
            verification_script,
            _script_hash: Mutex::new(None),
        })
    }
}
