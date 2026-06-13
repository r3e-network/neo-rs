//! Contract - matches C# Neo.SmartContract.Contract exactly

use neo_crypto::{Crypto, ECPoint};
use neo_error::CoreError;
use neo_primitives::ContractParameterType;
use neo_primitives::UInt160;
use neo_primitives::base58_check;
use neo_vm::script_builder::ScriptBuilder;
use std::sync::OnceLock;

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
    /// Returns `CoreError` if:
    /// - `public_keys` is empty
    /// - `public_keys.len()` exceeds 1024
    /// - `m` is not in range `1..=n`
    pub fn try_create_multi_sig_redeem_script(
        m: usize,
        public_keys: &[ECPoint],
    ) -> Result<Vec<u8>, CoreError> {
        // The redeem-script byte construction was hoisted into the
        // `neo-script-builder` crate (below neo-core); kept here for the
        // historical `Contract::try_create_multi_sig_redeem_script` path.
        neo_vm::script_builder::redeem_script::multi_sig_redeem_script_from_points(m, public_keys).map_err(Into::into)
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
            neo_config::ProtocolSettings::default_settings().address_version,
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
