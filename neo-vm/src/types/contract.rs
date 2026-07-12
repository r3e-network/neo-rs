//! Contract — a verification contract (script + parameter list + cached hash).
//!
//! This type was originally defined in `neo-execution` but has been moved here
//! because it is a pure data structure with no execution logic. Both
//! `neo-execution` (for witness verification) and `neo-wallets` (for account
//! contract management) need it, so it lives at the VM infrastructure layer
//! to avoid a circular dependency.

use crate::script_builder::ScriptBuilder;
use neo_crypto::{Crypto, ECPoint};
use neo_error::CoreError;
use neo_primitives::ContractParameterType;
use neo_primitives::UInt160;
use std::sync::OnceLock;

/// Represents a verification contract (matches C# `Neo.SmartContract.Contract`).
///
/// A `Contract` bundles the redeem script that verifies a witness with the
/// list of parameter types the script expects. The script hash (which is also
/// the account address) is computed lazily and cached.
#[derive(Clone, Debug)]
pub struct Contract {
    /// The verification (redeem) script.
    pub script: Vec<u8>,

    /// The parameter types the script expects on the evaluation stack.
    pub parameter_list: Vec<ContractParameterType>,

    /// Cached script hash (computed once, on first access).
    script_hash_cache: OnceLock<UInt160>,
}

impl Contract {
    /// Creates a new contract from a parameter list and redeem script.
    pub fn create(parameter_list: Vec<ContractParameterType>, redeem_script: Vec<u8>) -> Self {
        Self {
            script: redeem_script,
            parameter_list,
            script_hash_cache: OnceLock::new(),
        }
    }

    /// Constructs a contract with empty script and a pre-supplied hash.
    ///
    /// Used when the full script is not available but the hash is known
    /// (e.g. from a NEP-6 wallet file that only stores the script hash).
    pub fn create_with_hash(
        script_hash: UInt160,
        parameter_list: Vec<ContractParameterType>,
    ) -> Self {
        let contract = Self {
            script: Vec::new(),
            parameter_list,
            script_hash_cache: OnceLock::new(),
        };
        let _ = contract.script_hash_cache.set(script_hash);
        contract
    }

    /// Returns the script hash (RIPEMD-160 of SHA-256 of the script).
    ///
    /// This is also the account address of the contract.
    pub fn script_hash(&self) -> UInt160 {
        *self
            .script_hash_cache
            .get_or_init(|| UInt160::from(Crypto::hash160(&self.script)))
    }

    /// Creates a multi-sig verification contract.
    pub fn create_multi_sig_contract(m: usize, public_keys: &[ECPoint]) -> Self {
        match Self::try_create_multi_sig_contract(m, public_keys) {
            Ok(contract) => contract,
            Err(error) => {
                tracing::error!(target: "neo_vm", %error, "multi-sig contract construction failed");
                Self::create(Vec::new(), Vec::new())
            }
        }
    }

    /// Tries to create a multi-sig verification contract.
    ///
    /// # Errors
    ///
    /// Returns `CoreError` if the redeem script cannot be constructed from
    /// the supplied threshold and public keys.
    pub fn try_create_multi_sig_contract(
        m: usize,
        public_keys: &[ECPoint],
    ) -> Result<Self, CoreError> {
        let script = Self::try_create_multi_sig_redeem_script(m, public_keys)?;
        Ok(Self::create(
            vec![ContractParameterType::Signature; m],
            script,
        ))
    }

    /// Tries to create the multi-sig redeem script bytes.
    ///
    /// # Errors
    ///
    /// Returns `CoreError` if:
    /// - `public_keys` is empty
    /// - `public_keys.len()` exceeds 1024
    /// - `m` is not in range `1..=n`
    pub fn try_create_multi_sig_redeem_script(
        m: usize,
        public_keys: &[ECPoint],
    ) -> Result<Vec<u8>, CoreError> {
        crate::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            m,
            public_keys,
        )
        .map_err(Into::into)
    }

    /// Creates the multi-sig redeem script bytes.
    ///
    /// Prefer `try_create_multi_sig_redeem_script` for fallible construction.
    /// This compatibility wrapper logs invalid input and returns an empty
    /// script rather than panicking.
    #[inline]
    pub fn create_multi_sig_redeem_script(m: usize, public_keys: &[ECPoint]) -> Vec<u8> {
        match Self::try_create_multi_sig_redeem_script(m, public_keys) {
            Ok(script) => script,
            Err(error) => {
                tracing::error!(target: "neo_vm", %error, "multi-sig redeem script construction failed");
                Vec::new()
            }
        }
    }

    /// Creates a standard single-signature verification contract.
    pub fn create_signature_contract(public_key: ECPoint) -> Self {
        let script = Self::create_signature_redeem_script(public_key);
        let parameter_list = vec![ContractParameterType::Signature];
        Self::create(parameter_list, script)
    }

    /// Creates the redeem script bytes for a single-signature contract.
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

    /// Returns the address string for this contract, using the given address version.
    ///
    /// The address version comes from `ProtocolSettings.address_version`.
    /// Callers should pass it explicitly rather than having this type depend
    /// on `neo-config`.
    pub fn get_address(&self, address_version: u8) -> String {
        self.script_hash().to_address_with_version(address_version)
    }
}

#[cfg(test)]
#[path = "tests/contract.rs"]
mod tests;
