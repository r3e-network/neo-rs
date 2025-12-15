//! ApplicationEngine.Crypto - matches C# Neo.SmartContract.ApplicationEngine.Crypto.cs

use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::ApplicationEngine;
use neo_vm::execution_engine::ExecutionEngine;
use neo_vm::VmResult;
use sha2::{Digest, Sha256};

/// The price of CheckSig in GAS (1 << 15 = 32768 * 30 = 983040)
pub const CHECK_SIG_PRICE: i64 = 1 << 15;

/// The price of CheckMultisig in GAS (base price per signature)
pub const CHECK_MULTISIG_PRICE: i64 = 1 << 15;

impl ApplicationEngine {
    /// Verifies a signature using secp256r1 (NIST P-256)
    pub fn crypto_check_sig(&mut self) -> Result<bool, String> {
        // Neo VM calling convention: the first parameter is on top of the stack.
        // For standard signature witnesses, the invocation script pushes the signature and
        // the verification script pushes the public key, leaving `pubkey` on top.
        let public_key = self.pop_bytes()?;
        let signature = self.pop_bytes()?;

        let message_bytes = self.get_sign_data()?;

        Ok(self.verify_signature(&message_bytes, &public_key, &signature))
    }

    /// Verifies multiple signatures (m-of-n multisig)
    /// Stack order: pubkeys on top, signatures below
    /// Each can be either an Array of byte arrays, or an integer count followed by that many byte arrays
    pub fn crypto_check_multisig(&mut self) -> Result<bool, String> {
        // Pop public keys first (top of stack)
        let public_keys = self.pop_sig_elements()?;
        let n = public_keys.len();
        if n == 0 || n > 1024 {
            return Err("Invalid public key count".to_string());
        }

        // Pop signatures second
        let signatures = self.pop_sig_elements()?;
        let m = signatures.len();
        if m == 0 || m > n {
            return Err("Invalid signature count".to_string());
        }

        let message_bytes = self.get_sign_data()?;

        let mut verified = 0;
        let mut key_index = 0;

        for signature in &signatures {
            while key_index < public_keys.len() {
                if self.verify_signature(&message_bytes, &public_keys[key_index], signature) {
                    verified += 1;
                    key_index += 1;
                    break;
                }
                key_index += 1;
            }
            // Early exit if remaining signatures exceed remaining keys
            if m - verified > n - key_index {
                return Ok(false);
            }
        }

        Ok(verified >= m)
    }

    fn get_sign_data(&self) -> Result<Vec<u8>, String> {
        let container = self
            .get_script_container()
            .ok_or_else(|| "No script container available".to_string())?;
        let hash = container.hash().map_err(|e| e.to_string())?;
        let network = self.protocol_settings().network;

        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&network.to_le_bytes());
        sign_data.extend_from_slice(&hash.as_bytes());
        Ok(sign_data)
    }

    /// Pop signature elements from stack - handles both Array format and N+items format
    fn pop_sig_elements(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let item = self.pop()?;

        match &item {
            neo_vm::stack_item::StackItem::Array(arr) => {
                // Array format: extract all byte arrays
                let items = arr.items();
                let mut result = Vec::with_capacity(items.len());
                for item in items {
                    result.push(item.as_bytes().map_err(|e| e.to_string())?);
                }
                Ok(result)
            }
            _ => {
                // Integer format: pop N items
                let count = item
                    .as_int()
                    .map_err(|e| e.to_string())?
                    .try_into()
                    .map_err(|_| "Count out of range")?;
                let count: usize = count;
                if count == 0 || count > 1024 {
                    return Err("Invalid element count".to_string());
                }
                let mut result = Vec::with_capacity(count);
                for _ in 0..count {
                    result.push(self.pop_bytes()?);
                }
                Ok(result)
            }
        }
    }

    /// SHA256 hash
    pub fn crypto_sha256(&mut self) -> Result<(), String> {
        let data = self.pop_bytes()?;
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let result = hasher.finalize();
        self.push_bytes(result.to_vec())
    }

    /// RIPEMD160 hash
    pub fn crypto_ripemd160(&mut self) -> Result<(), String> {
        let data = self.pop_bytes()?;
        use ripemd::Digest as RipemdDigest;
        use ripemd::Ripemd160;
        let mut hasher = Ripemd160::new();
        hasher.update(&data);
        let result = hasher.finalize();
        self.push_bytes(result.to_vec())
    }

    /// Verifies a signature using secp256r1
    fn verify_signature(&self, message: &[u8], public_key: &[u8], signature: &[u8]) -> bool {
        use p256::ecdsa::{signature::Verifier, Signature, VerifyingKey};

        if signature.len() != 64 || public_key.len() != 33 {
            return false;
        }

        let Ok(verifying_key) = VerifyingKey::from_sec1_bytes(public_key) else {
            return false;
        };

        let Ok(sig) = Signature::from_slice(signature) else {
            return false;
        };

        verifying_key.verify(message, &sig).is_ok()
    }
}

// Handler functions for syscall registration
fn crypto_check_sig_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    match app.crypto_check_sig() {
        Ok(result) => app
            .push_boolean(result)
            .map_err(|e| neo_vm::VmError::InteropService {
                service: "System.Crypto.CheckSig".to_string(),
                error: e,
            }),
        Err(e) => Err(neo_vm::VmError::InteropService {
            service: "System.Crypto.CheckSig".to_string(),
            error: e,
        }),
    }
}

fn crypto_check_multisig_handler(
    app: &mut ApplicationEngine,
    _engine: &mut ExecutionEngine,
) -> VmResult<()> {
    match app.crypto_check_multisig() {
        Ok(result) => app
            .push_boolean(result)
            .map_err(|e| neo_vm::VmError::InteropService {
                service: "System.Crypto.CheckMultisig".to_string(),
                error: e,
            }),
        Err(e) => Err(neo_vm::VmError::InteropService {
            service: "System.Crypto.CheckMultisig".to_string(),
            error: e,
        }),
    }
}

/// Registers crypto-related interop services
pub(crate) fn register_crypto_interops(engine: &mut ApplicationEngine) -> VmResult<()> {
    engine.register_host_service(
        "System.Crypto.CheckSig",
        CHECK_SIG_PRICE,
        CallFlags::NONE,
        crypto_check_sig_handler,
    )?;
    engine.register_host_service(
        "System.Crypto.CheckMultisig",
        CHECK_MULTISIG_PRICE,
        CallFlags::NONE,
        crypto_check_multisig_handler,
    )?;
    Ok(())
}
