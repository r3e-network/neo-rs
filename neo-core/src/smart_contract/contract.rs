//! Contract - matches C# Neo.SmartContract.Contract exactly

use crate::UInt160;
use crate::cryptography::{ECPoint, NeoHash};
use crate::error::CoreError;
use crate::smart_contract::ContractParameterType;
use neo_vm::ScriptBuilder;
use std::sync::OnceLock;

/// Error type for multi-signature contract creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiSigError {
    /// The m parameter is not in valid range (1..=n).
    InvalidM { m: usize, n: usize },
    /// No public keys provided.
    EmptyPublicKeys,
    /// Too many public keys (max 1024).
    TooManyPublicKeys { n: usize },
    /// Failed to build the syscall portion of the script.
    ScriptBuilder(String),
}

impl std::fmt::Display for MultiSigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidM { m, n } => write!(f, "Invalid multi-sig parameters: m={}, n={}", m, n),
            Self::EmptyPublicKeys => write!(f, "No public keys provided for multi-sig contract"),
            Self::TooManyPublicKeys { n } => {
                write!(f, "Too many public keys: {} (max 1024)", n)
            }
            Self::ScriptBuilder(err) => write!(f, "Failed to build contract script: {err}"),
        }
    }
}

impl std::error::Error for MultiSigError {}

impl From<MultiSigError> for CoreError {
    fn from(err: MultiSigError) -> Self {
        CoreError::invalid_operation(err.to_string())
    }
}

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
            .get_or_init(|| UInt160::from(NeoHash::hash160(&self.script)))
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
    /// Returns `MultiSigError` if:
    /// - `public_keys` is empty
    /// - `public_keys.len()` exceeds 1024
    /// - `m` is not in range `1..=n`
    pub fn try_create_multi_sig_redeem_script(
        m: usize,
        public_keys: &[ECPoint],
    ) -> Result<Vec<u8>, MultiSigError> {
        let n = public_keys.len();
        if n == 0 {
            return Err(MultiSigError::EmptyPublicKeys);
        }
        if n > 1024 {
            return Err(MultiSigError::TooManyPublicKeys { n });
        }
        if !(1..=n).contains(&m) {
            return Err(MultiSigError::InvalidM { m, n });
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
            .map_err(|err| MultiSigError::ScriptBuilder(err.to_string()))?;

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
        let mut data = Vec::with_capacity(1 + crate::neo_config::ADDRESS_SIZE + 4);
        // Default address version (0x35) matches ProtocolSettings::default
        data.push(crate::protocol_settings::ProtocolSettings::default_settings().address_version);
        data.extend_from_slice(&self.script_hash().to_bytes());

        let checksum = crate::cryptography::crypto_utils::NeoHash::hash256(&data);
        data.extend_from_slice(&checksum[..4]);

        crate::cryptography::crypto_utils::Base58::encode(&data)
    }
}
