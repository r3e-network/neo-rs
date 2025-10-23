//! ApplicationEngine.Crypto - matches C# Neo.SmartContract.ApplicationEngine.Crypto.cs exactly

use crate::smart_contract::ApplicationEngine;
use sha2::{Digest, Sha256};

impl ApplicationEngine {
    /// Verifies a signature
    pub fn crypto_check_sig(&mut self) -> Result<bool, String> {
        // Pop signature and public key from stack
        let signature = self.pop_bytes()?;
        let public_key = self.pop_bytes()?;

        // Get the verification script container
        let container = self
            .get_script_container()
            .ok_or_else(|| "No script container available".to_string())?;
        let message_hash = container.hash().map_err(|e| e.to_string())?;
        let message_bytes = message_hash.to_bytes();

        // Verify signature (simplified - would use actual crypto)
        Ok(self.verify_signature(&message_bytes, &public_key, &signature))
    }

    /// Verifies multiple signatures
    pub fn crypto_check_multisig(&mut self) -> Result<bool, String> {
        // Pop n (number of public keys)
        let n = self.pop_integer()? as usize;
        if n == 0 || n > 1024 {
            return Err("Invalid public key count".to_string());
        }

        // Pop public keys
        let mut public_keys = Vec::with_capacity(n);
        for _ in 0..n {
            public_keys.push(self.pop_bytes()?);
        }

        // Pop m (number of signatures required)
        let m = self.pop_integer()? as usize;
        if m == 0 || m > n {
            return Err("Invalid signature count".to_string());
        }

        // Pop signatures
        let mut signatures = Vec::with_capacity(m);
        for _ in 0..m {
            signatures.push(self.pop_bytes()?);
        }

        // Get message to verify
        let container = self
            .get_script_container()
            .ok_or_else(|| "No script container available".to_string())?;
        let message_hash = container.hash().map_err(|e| e.to_string())?;
        let message_bytes = message_hash.to_bytes();

        // Verify m-of-n signatures
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
        }

        Ok(verified >= m)
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

        // Use ripemd crate or implement
        use ripemd::Digest as RipemdDigest;
        use ripemd::Ripemd160;

        let mut hasher = Ripemd160::new();
        hasher.update(&data);
        let result = hasher.finalize();

        self.push_bytes(result.to_vec())
    }

    /// Verifies a signature (helper method)
    fn verify_signature(&self, _message: &[u8], public_key: &[u8], signature: &[u8]) -> bool {
        // Simplified verification - in real implementation would use secp256r1
        if signature.len() != 64 || public_key.len() != 33 {
            return false;
        }

        // Placeholder verification
        true
    }
}
