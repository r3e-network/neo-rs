//! Pure-Rust in-memory PKCS#11 backend for tests.
//!
//! The mock holds an ephemeral signing key that never touches real hardware,
//! records every signing call for mechanism and digest assertions, and
//! rejects any slot, token, key-label, PIN, or mechanism mismatch.

use super::backend::{Pkcs11Backend, Pkcs11KeyQuery, Pkcs11Mechanism, Pkcs11SignCall};
use anyhow::{Result, bail};
use neo_crypto::Secp256r1Crypto;
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::sync::Arc;

/// One recorded backend signing call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedSign {
    pub(crate) slot_id: u64,
    pub(crate) token_label: String,
    pub(crate) key_label: String,
    pub(crate) pin: Vec<u8>,
    pub(crate) mechanism: Pkcs11Mechanism,
    pub(crate) data: Vec<u8>,
}

/// In-memory PKCS#11 token with a single P-256 key pair.
pub(crate) struct MockP11Backend {
    slot_id: u64,
    token_label: String,
    key_label: String,
    private_key: [u8; 32],
    public_key: Vec<u8>,
    pin: Vec<u8>,
    allowed_mechanisms: BTreeSet<Pkcs11Mechanism>,
    calls: Arc<Mutex<Vec<RecordedSign>>>,
}

impl Clone for MockP11Backend {
    fn clone(&self) -> Self {
        Self {
            slot_id: self.slot_id,
            token_label: self.token_label.clone(),
            key_label: self.key_label.clone(),
            private_key: self.private_key,
            public_key: self.public_key.clone(),
            pin: self.pin.clone(),
            allowed_mechanisms: self.allowed_mechanisms.clone(),
            calls: Arc::clone(&self.calls),
        }
    }
}

impl MockP11Backend {
    /// Creates a mock token whose key pair is derived from `private_key`.
    pub(crate) fn new(
        slot_id: u64,
        token_label: &str,
        key_label: &str,
        private_key: &[u8; 32],
        pin: &str,
    ) -> Self {
        let public_key =
            Secp256r1Crypto::derive_public_key(private_key).expect("derive mock public key");
        Self {
            slot_id,
            token_label: token_label.to_string(),
            key_label: key_label.to_string(),
            private_key: *private_key,
            public_key,
            pin: pin.as_bytes().to_vec(),
            allowed_mechanisms: BTreeSet::from([
                Pkcs11Mechanism::Ecdsa,
                Pkcs11Mechanism::EcdsaSha256,
            ]),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Restricts the mechanisms this token advertises.
    pub(crate) fn restrict_mechanisms(mut self, mechanisms: &[Pkcs11Mechanism]) -> Self {
        self.allowed_mechanisms = mechanisms.iter().copied().collect();
        self
    }

    /// Returns the compressed public key held by the mock token.
    pub(crate) fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Returns every recorded signing call in order.
    pub(crate) fn recorded_signs(&self) -> Vec<RecordedSign> {
        self.calls.lock().clone()
    }

    fn verify_query(&self, slot_id: u64, token_label: &str, key_label: &str) -> Result<()> {
        if slot_id != self.slot_id || token_label != self.token_label || key_label != self.key_label
        {
            bail!(
                "mock PKCS#11 backend has no token matching slot {slot_id}, label '{token_label}', key '{key_label}'"
            );
        }
        Ok(())
    }
}

impl Pkcs11Backend for MockP11Backend {
    fn supports_mechanism(&self, slot_id: u64, mechanism: Pkcs11Mechanism) -> bool {
        slot_id == self.slot_id && self.allowed_mechanisms.contains(&mechanism)
    }

    fn export_public_key(&self, query: &Pkcs11KeyQuery) -> Result<Vec<u8>> {
        self.verify_query(query.slot_id, &query.token_label, &query.key_label)?;
        Ok(self.public_key.clone())
    }

    fn sign(&self, call: &Pkcs11SignCall) -> Result<Vec<u8>> {
        self.verify_query(call.slot_id, &call.token_label, &call.key_label)?;
        if call.pin != self.pin {
            bail!("mock PKCS#11 backend rejected the user PIN");
        }
        if !self.allowed_mechanisms.contains(&call.mechanism) {
            bail!(
                "mock PKCS#11 backend does not allow mechanism {:?}",
                call.mechanism
            );
        }
        let signature = match call.mechanism {
            Pkcs11Mechanism::EcdsaSha256 => Secp256r1Crypto::sign(&call.data, &self.private_key)?,
            Pkcs11Mechanism::Ecdsa => {
                let digest: [u8; 32] = call
                    .data
                    .as_slice()
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("CKM_ECDSA requires a 32-byte digest"))?;
                Secp256r1Crypto::sign_prehash(&digest, &self.private_key)?
            }
        };
        self.calls.lock().push(RecordedSign {
            slot_id: call.slot_id,
            token_label: call.token_label.clone(),
            key_label: call.key_label.clone(),
            pin: call.pin.clone(),
            mechanism: call.mechanism,
            data: call.data.clone(),
        });
        Ok(signature.to_vec())
    }
}
