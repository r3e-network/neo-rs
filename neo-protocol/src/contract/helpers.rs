// Copyright (c) 2024 R3E Network
// Licensed under the MIT License. See LICENSE file for details.

//! Contract construction helpers — matches C# `Neo.SmartContract.Contract`.
//!
//! Migrated from `neo-core::smart_contract::contract`. The only structural
//! change is the error type: the crate-local `CoreError` is replaced by a
//! self-contained [`ContractError`] so this module depends solely on the
//! permitted foundation crates (`neo-crypto`, `neo-primitives`, `neo-vm` via
//! [`crate::builders::ScriptBuilder`]) and never on `neo-core`.

use crate::builders::ScriptBuilder;
use crate::protocol_settings::ProtocolSettings;
use neo_crypto::{Crypto, ECPoint};
use neo_primitives::base58_check;
use neo_primitives::{ContractParameterType, UInt160};
use std::sync::OnceLock;

/// A self-contained error for contract construction failures.
///
/// Replaces the historical `CoreError::invalid_operation` usage so the protocol
/// layer stays independent of `neo-core`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractError {
    message: String,
}

impl ContractError {
    /// Creates an error describing an invalid operation.
    pub fn invalid_operation<S: Into<String>>(message: S) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Returns the underlying message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ContractError {}

/// Represents a contract that can be invoked (matches C# Contract)
#[derive(Clone, Debug)]
pub struct Contract {
    /// The script of the contract
    pub script: Vec<u8>,

    /// The parameters of the contract
    pub parameter_list: Vec<ContractParameterType>,

    /// Cached script hash
    script_hash_cache: OnceLock<UInt160>,
}

impl Contract {
    /// Creates a new instance
    pub fn create(parameter_list: Vec<ContractParameterType>, redeem_script: Vec<u8>) -> Self {
        Self {
            script: redeem_script,
            parameter_list,
            script_hash_cache: OnceLock::new(),
        }
    }

    /// Constructs a special contract with empty script
    pub fn create_with_hash(
        script_hash: UInt160,
        parameter_list: Vec<ContractParameterType>,
    ) -> Self {
        let contract = Self {
            script: Vec::new(),
            parameter_list,
            script_hash_cache: OnceLock::new(),
        };
        // Pre-populate the cache with the provided hash
        let _ = contract.script_hash_cache.set(script_hash);
        contract
    }

    /// Gets the hash of the contract
    pub fn script_hash(&self) -> UInt160 {
        *self
            .script_hash_cache
            .get_or_init(|| UInt160::from(Crypto::hash160(&self.script)))
    }

    /// Creates a multi-sig contract
    pub fn create_multi_sig_contract(m: usize, public_keys: &[ECPoint]) -> Self {
        let script = Self::create_multi_sig_redeem_script(m, public_keys);
        let parameter_list = vec![ContractParameterType::Signature; m];

        Self::create(parameter_list, script)
    }

    /// Creates the script of a multi-sig contract.
    ///
    /// # Errors
    ///
    /// Returns [`ContractError`] if:
    /// - `public_keys` is empty
    /// - `public_keys.len()` exceeds 1024
    /// - `m` is not in range `1..=n`
    pub fn try_create_multi_sig_redeem_script(
        m: usize,
        public_keys: &[ECPoint],
    ) -> Result<Vec<u8>, ContractError> {
        let n = public_keys.len();
        if n == 0 {
            return Err(ContractError::invalid_operation(
                "No public keys provided for multi-sig contract",
            ));
        }
        if n > 1024 {
            return Err(ContractError::invalid_operation(format!(
                "Too many public keys: {} (max 1024)",
                n
            )));
        }
        if !(1..=n).contains(&m) {
            return Err(ContractError::invalid_operation(format!(
                "Invalid multi-sig parameters: m={}, n={}",
                m, n
            )));
        }

        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(m as i64);

        let mut sorted_keys = public_keys.to_vec();
        sorted_keys.sort();
        for key in sorted_keys.iter() {
            let encoded = key.encode_point(true).unwrap_or_else(|_| key.to_bytes());
            builder.emit_push(&encoded);
        }

        builder.emit_push_int(n as i64);
        builder
            .emit_syscall("System.Crypto.CheckMultisig")
            .map_err(|err| {
                ContractError::invalid_operation(format!(
                    "Failed to build contract script: {}",
                    err
                ))
            })?;

        Ok(builder.to_array())
    }

    /// Creates the script of a multi-sig contract (panics on invalid input).
    ///
    /// Prefer `try_create_multi_sig_redeem_script` for fallible construction.
    #[inline]
    pub fn create_multi_sig_redeem_script(m: usize, public_keys: &[ECPoint]) -> Vec<u8> {
        Self::try_create_multi_sig_redeem_script(m, public_keys)
            .expect("Invalid multi-sig parameters")
    }

    /// Creates a signature contract
    pub fn create_signature_contract(public_key: ECPoint) -> Self {
        let script = Self::create_signature_redeem_script(public_key);
        let parameter_list = vec![ContractParameterType::Signature];

        Self::create(parameter_list, script)
    }

    /// Creates the script of a signature contract.
    pub fn create_signature_redeem_script(public_key: ECPoint) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        let encoded = public_key
            .encode_point(true)
            .unwrap_or_else(|_| public_key.to_bytes());
        builder.emit_push(&encoded);
        if let Err(err) = builder.emit_syscall("System.Crypto.CheckSig") {
            tracing::error!("failed to emit System.Crypto.CheckSig syscall: {err}");
            return Vec::new();
        }
        builder.to_array()
    }

    /// Gets the address of the contract
    pub fn get_address(&self) -> String {
        base58_check::encode_address_payload(
            ProtocolSettings::default_settings().address_version,
            &self.script_hash().to_array(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_address_uses_default_protocol_base58_check_payload() {
        let script_hash = UInt160::from_bytes(&[0x31; UInt160::LENGTH]).unwrap();
        let contract = Contract::create_with_hash(script_hash, Vec::new());

        assert_eq!(contract.get_address(), script_hash.to_address());
    }
}
