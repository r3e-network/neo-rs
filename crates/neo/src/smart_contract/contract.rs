//! Contract - matches C# Neo.SmartContract.Contract exactly

use crate::cryptography::crypto_utils::{ECPoint, NeoHash};
use crate::smart_contract::ContractParameterType;
use crate::UInt160;
use neo_vm::ScriptBuilder;
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
        *self.script_hash_cache.get_or_init(|| {
            UInt160::from_bytes(&NeoHash::hash160(&self.script)).expect("hash160 produces 20 bytes")
        })
    }

    /// Creates a multi-sig contract
    pub fn create_multi_sig_contract(m: usize, public_keys: &[ECPoint]) -> Self {
        let script = Self::create_multi_sig_redeem_script(m, public_keys);
        let parameter_list = vec![ContractParameterType::Signature; m];

        Self::create(parameter_list, script)
    }

    /// Creates the script of a multi-sig contract.
    pub fn create_multi_sig_redeem_script(m: usize, public_keys: &[ECPoint]) -> Vec<u8> {
        let n = public_keys.len();
        if !(1..=n).contains(&m) || n == 0 || n > 1024 {
            panic!("Invalid multi-sig parameters: m={}, n={}", m, n);
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
            .expect("emit_syscall failed");

        builder.to_array()
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
        builder
            .emit_syscall("System.Crypto.CheckSig")
            .expect("emit_syscall failed");
        builder.to_array()
    }

    /// Gets the address of the contract
    pub fn get_address(&self) -> String {
        let mut data = Vec::with_capacity(1 + crate::neo_config::ADDRESS_SIZE + 4);
        // Default address version (0x35) matches ProtocolSettings::default
        data.push(crate::protocol_settings::ProtocolSettings::default_settings().address_version);
        data.extend_from_slice(&self.script_hash().to_bytes());

        let checksum = crate::cryptography::crypto_utils::NeoHash::hash256(&data);
        data.extend_from_slice(&checksum[..4]);

        crate::cryptography::crypto_utils::Base58::encode(&data)
    }
}
