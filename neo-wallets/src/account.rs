//! Wallet account abstraction and standard implementation.
//!
//! Mirrors the behaviour of the C# `Neo.Wallets.WalletAccount` hierarchy as
//! ported into `neo-core`'s `wallets::wallet_account`, re-based onto the
//! standalone dependency set. Two types that live outside the allowed
//! dependency cone are represented here:
//!
//! * [`Contract`] — a lightweight stand-in for the `neo-smart-contract`
//!   `Contract`. It carries just enough (verification script + parameter names +
//!   deployed flag) for wallet account bookkeeping and NEP-6 (de)serialization.
//!   Reconciling this with the canonical contract type is a task 89 alignment item.
//! * [`Witness`] — a concrete witness (invocation + verification script) that
//!   implements the `neo_primitives::Witness` trait.
//!
//! # Signing adaptation
//!
//! `neo-core`'s `WalletAccount::create_witness` took a `&Transaction`. Because
//! transaction payloads live in forbidden crates, this port signs a
//! pre-computed *signature preimage* (`sign_data`) instead. Callers obtain that
//! preimage via `neo_protocol::get_sign_data_vec`, keeping the witness bytes
//! identical while removing the ledger/protocol-payload coupling.

use crate::helper::Helper;
use crate::key_pair::KeyPair;
use crate::wallet::{WalletError, WalletResult};
use async_trait::async_trait;
use neo_primitives::UInt160;
use neo_protocol::ProtocolSettings;
use neo_smart_contract::helper::Helper as ContractHelper;
use std::sync::Arc;

// Re-export types from their canonical locations
pub use neo_protocol::witness::Witness;
pub use neo_smart_contract::contract::Contract;

/// `PUSHDATA1` opcode byte (matches `neo_vm::OpCode::PUSHDATA1`).
const OP_PUSHDATA1: u8 = 0x0C;

/// Common interface shared by all wallet-backed accounts.
#[async_trait]
pub trait WalletAccount: Send + Sync {
    /// Script hash identifying the account on chain.
    fn script_hash(&self) -> UInt160;

    /// Human readable address corresponding to the script hash.
    fn address(&self) -> String;

    /// Optional user supplied label.
    fn label(&self) -> Option<&str>;

    /// Updates the stored label.
    fn set_label(&mut self, label: Option<String>);

    /// Indicates whether this account is marked as the wallet default.
    fn is_default(&self) -> bool;

    /// Updates the default flag.
    fn set_is_default(&mut self, is_default: bool);

    /// Returns `true` when the private key material is currently locked.
    fn is_locked(&self) -> bool;

    /// Returns whether the account owns signing material (key or NEP-2 payload).
    fn has_key(&self) -> bool;

    /// Returns the decrypted key pair if present.
    fn get_key(&self) -> Option<KeyPair>;

    /// Returns the contract (if any) bound to this account.
    fn contract(&self) -> Option<&Contract>;

    /// Updates the stored contract.
    fn set_contract(&mut self, contract: Option<Contract>);

    /// Underlying protocol settings used for address conversions.
    fn protocol_settings(&self) -> &Arc<ProtocolSettings>;

    /// Attempts to unlock the encrypted key material with the supplied password.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when decryption encounters a fatal error.
    fn unlock(&mut self, password: &str) -> WalletResult<bool>;

    /// Locks the account (when a NEP-2 key is available).
    fn lock(&mut self);

    /// Validates that the supplied password can decrypt the encrypted key.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError`] when verification encounters a fatal error.
    fn verify_password(&self, password: &str) -> WalletResult<bool>;

    /// Exports the key material in WIF format.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::AccountLocked`] when no decrypted key is available.
    fn export_wif(&self) -> WalletResult<String>;

    /// Exports the key material encoded as NEP-2.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::AccountLocked`] when the account has no key material.
    fn export_nep2(&self, password: &str) -> WalletResult<String>;

    /// Creates a witness for the supplied signature preimage using the account credentials.
    ///
    /// Single-key accounts produce the canonical single-signature witness
    /// (66-byte invocation script, 40-byte verification script). Multi-signature
    /// and deployed-contract scripts fail closed: use
    /// [`crate::multisig::MultiSigWitnessBuilder`] to collect the m required
    /// signatures instead of silently emitting an invalid witness.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::AccountLocked`] when the key is unavailable, or
    /// [`WalletError::SigningFailed`] when signing fails or the verification
    /// script cannot be satisfied by a single key.
    fn create_witness(&self, sign_data: &[u8]) -> WalletResult<Witness>;
}

/// Concrete wallet account implementation providing the same ergonomics as the
/// C# `WalletAccount` class.
#[derive(Debug, Clone)]
pub struct StandardWalletAccount {
    script_hash: UInt160,
    label: Option<String>,
    is_default: bool,
    is_locked: bool,
    contract: Option<Contract>,
    key_pair: Option<KeyPair>,
    nep2_key: Option<String>,
    protocol_settings: Arc<ProtocolSettings>,
}

impl StandardWalletAccount {
    /// Creates an account backed by the provided key pair.
    #[must_use]
    pub fn new_with_key(
        key_pair: KeyPair,
        contract: Option<Contract>,
        protocol_settings: Arc<ProtocolSettings>,
        nep2_key: Option<String>,
    ) -> Self {
        let script_hash = contract
            .as_ref()
            .map_or_else(|| key_pair.get_script_hash(), Contract::script_hash);

        Self {
            script_hash,
            label: None,
            is_default: false,
            is_locked: false,
            contract,
            key_pair: Some(key_pair),
            nep2_key,
            protocol_settings,
        }
    }

    /// Creates a watch-only account for the supplied script hash.
    #[must_use]
    pub fn new_watch_only(
        script_hash: UInt160,
        contract: Option<Contract>,
        protocol_settings: Arc<ProtocolSettings>,
    ) -> Self {
        Self {
            script_hash,
            label: None,
            is_default: false,
            is_locked: false,
            contract,
            key_pair: None,
            nep2_key: None,
            protocol_settings,
        }
    }

    /// Creates an account backed solely by a NEP-2 encrypted key.
    #[must_use]
    pub fn new_from_encrypted(
        script_hash: UInt160,
        nep2_key: String,
        contract: Option<Contract>,
        protocol_settings: Arc<ProtocolSettings>,
    ) -> Self {
        Self {
            script_hash,
            label: None,
            is_default: false,
            is_locked: true,
            contract,
            key_pair: None,
            nep2_key: Some(nep2_key),
            protocol_settings,
        }
    }

    /// Returns the stored NEP-2 key (if any).
    #[must_use]
    pub fn nep2_key(&self) -> Option<&str> {
        self.nep2_key.as_deref()
    }

    /// Replaces the stored NEP-2 payload.
    pub fn set_nep2_key(&mut self, nep2: Option<String>) {
        self.nep2_key = nep2;
    }
}

impl WalletAccount for StandardWalletAccount {
    fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    fn address(&self) -> String {
        Helper::to_address(&self.script_hash, self.protocol_settings.address_version)
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    fn set_label(&mut self, label: Option<String>) {
        self.label = label;
    }

    fn is_default(&self) -> bool {
        self.is_default
    }

    fn set_is_default(&mut self, is_default: bool) {
        self.is_default = is_default;
    }

    fn is_locked(&self) -> bool {
        self.is_locked
    }

    fn has_key(&self) -> bool {
        self.key_pair.is_some() || self.nep2_key.is_some()
    }

    fn get_key(&self) -> Option<KeyPair> {
        self.key_pair.clone()
    }

    fn contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }

    fn set_contract(&mut self, contract: Option<Contract>) {
        if let Some(contract) = contract {
            self.script_hash = contract.script_hash();
            self.contract = Some(contract);
        } else {
            self.contract = None;
        }
    }

    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        &self.protocol_settings
    }

    fn unlock(&mut self, password: &str) -> WalletResult<bool> {
        if !self.is_locked {
            return Ok(true);
        }

        let Some(nep2) = &self.nep2_key else {
            return Ok(false);
        };

        let version = self.protocol_settings.address_version;
        match KeyPair::from_nep2_string(nep2, password, version) {
            Ok(key_pair) => {
                self.key_pair = Some(key_pair);
                self.is_locked = false;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn lock(&mut self) {
        if self.nep2_key.is_some() {
            self.key_pair = None;
            self.is_locked = true;
        }
    }

    fn verify_password(&self, password: &str) -> WalletResult<bool> {
        if let Some(nep2) = &self.nep2_key {
            let version = self.protocol_settings.address_version;
            Ok(KeyPair::from_nep2_string(nep2, password, version).is_ok())
        } else {
            Ok(self.key_pair.is_some())
        }
    }

    fn export_wif(&self) -> WalletResult<String> {
        match &self.key_pair {
            Some(key) => Ok(key.to_wif()),
            None => Err(WalletError::AccountLocked),
        }
    }

    fn export_nep2(&self, password: &str) -> WalletResult<String> {
        if let Some(nep2) = &self.nep2_key {
            return Ok(nep2.clone());
        }

        let key = self.key_pair.as_ref().ok_or(WalletError::AccountLocked)?;
        let version = self.protocol_settings.address_version;
        key.to_nep2(password, version)
    }

    fn create_witness(&self, sign_data: &[u8]) -> WalletResult<Witness> {
        let key = self.key_pair.as_ref().ok_or(WalletError::AccountLocked)?;

        // Fail closed on verification scripts a single key cannot satisfy:
        // an m-of-n script needs m signature parameters (collected through
        // `MultiSigWitnessBuilder`), and arbitrary deployed scripts have no
        // known invocation encoding in the wallet layer.
        if let Some(contract) = &self.contract {
            if ContractHelper::is_multi_sig_contract(&contract.script) {
                return Err(WalletError::SigningFailed(
                    "multi-signature contract requires collecting m signatures via MultiSigWitnessBuilder"
                        .to_string(),
                ));
            }
            if !ContractHelper::is_signature_contract(&contract.script) {
                return Err(WalletError::SigningFailed(
                    "contract verification script is not a supported single-signature script"
                        .to_string(),
                ));
            }
            if contract.script != key.get_verification_script() {
                return Err(WalletError::SigningFailed(
                    "single-signature contract public key does not match the account key"
                        .to_string(),
                ));
            }
        }

        let signature = key.sign(sign_data)?;

        let verification_script = if let Some(contract) = &self.contract {
            contract.script.clone()
        } else {
            key.get_verification_script()
        };

        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OP_PUSHDATA1);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);

        Ok(Witness::new_with_scripts(invocation, verification_script))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::ECPoint;

    fn settings() -> Arc<ProtocolSettings> {
        Arc::new(ProtocolSettings::default_settings())
    }

    #[test]
    fn watch_only_account_has_no_key() {
        let hash = UInt160::from_script(&[1, 2, 3]);
        let account = StandardWalletAccount::new_watch_only(hash, None, settings());
        assert!(!account.has_key());
        assert!(account.get_key().is_none());
        assert_eq!(account.script_hash(), hash);
    }

    #[test]
    fn keyed_account_lock_unlock_round_trip() {
        let key = KeyPair::generate().unwrap();
        let version = settings().address_version;
        let nep2 = key.to_nep2("pw", version).unwrap();
        let mut account = StandardWalletAccount::new_from_encrypted(
            key.get_script_hash(),
            nep2,
            None,
            settings(),
        );

        assert!(account.is_locked());
        assert!(account.has_key());
        assert!(account.get_key().is_none());
        // A correct password decrypts the NEP-2 payload and clears the lock.
        assert!(account.unlock("pw").unwrap());
        assert!(!account.is_locked());
        assert!(account.get_key().is_some());
        // A wrong password leaves the account locked.
        account.lock();
        assert!(!account.unlock("bad").unwrap());
        assert!(account.is_locked());
        account.lock();
        assert!(account.is_locked());
        assert!(account.get_key().is_none());
    }

    #[test]
    fn create_witness_uses_signature_redeem_script() {
        let key = KeyPair::generate().unwrap();
        let expected_script = key.get_verification_script();
        let account = StandardWalletAccount::new_with_key(key, None, settings(), None);
        let witness = account.create_witness(b"sign-me").unwrap();
        assert_eq!(witness.verification_script, expected_script);
        // invocation = PUSHDATA1 <64> <signature>
        assert_eq!(witness.invocation_script[0], OP_PUSHDATA1);
        assert_eq!(witness.invocation_script[1], 64);
        assert_eq!(witness.invocation_script.len(), 66);
    }

    #[test]
    fn signature_contract_script_hash_matches_key() {
        let key = KeyPair::generate().unwrap();
        let public_key = ECPoint::from_bytes(&key.public_key()).unwrap();
        let contract = Contract::create_signature_contract(public_key);
        assert_eq!(contract.script_hash(), key.get_script_hash());
    }

    #[test]
    fn create_witness_accepts_signature_contract_with_canonical_byte_format() {
        let key = KeyPair::generate().unwrap();
        let contract = Contract::create_signature_contract(key.get_public_key_point().unwrap());
        let account = StandardWalletAccount::new_with_key(key, Some(contract), settings(), None);
        let witness = account.create_witness(b"sign-me").unwrap();
        // Byte-level contract with the ledger verifier: 66-byte invocation
        // (PUSHDATA1 + length + 64-byte signature) and 40-byte verification
        // script (PUSHDATA1 + 33-byte key + SYSCALL + 4-byte interop hash).
        assert_eq!(witness.invocation_script.len(), 66);
        assert_eq!(witness.invocation_script[0], OP_PUSHDATA1);
        assert_eq!(witness.invocation_script[1], 64);
        assert_eq!(witness.verification_script.len(), 40);
    }

    #[test]
    fn create_witness_rejects_signature_contract_for_a_different_key() {
        let key = KeyPair::generate().unwrap();
        let other = KeyPair::generate().unwrap();
        let contract = Contract::create_signature_contract(other.get_public_key_point().unwrap());
        let account = StandardWalletAccount::new_with_key(key, Some(contract), settings(), None);

        let result = account.create_witness(b"sign-me");
        assert!(matches!(result, Err(WalletError::SigningFailed(_))));
    }

    #[test]
    fn create_witness_fails_closed_for_multisig_contract() {
        let keys: Vec<KeyPair> = (0..2).map(|_| KeyPair::generate().unwrap()).collect();
        let points: Vec<ECPoint> = keys
            .iter()
            .map(|key| key.get_public_key_point().unwrap())
            .collect();
        let contract = Contract::create_multi_sig_contract(2, &points);
        let account =
            StandardWalletAccount::new_with_key(keys[0].clone(), Some(contract), settings(), None);
        let result = account.create_witness(b"sign-me");
        assert!(matches!(result, Err(WalletError::SigningFailed(_))));
    }

    #[test]
    fn create_witness_fails_closed_for_unsupported_contract_script() {
        let key = KeyPair::generate().unwrap();
        // A deployed-contract entry with an empty script cannot be
        // satisfied by any invocation this single-key path can produce.
        let contract = Contract::create(vec![], Vec::new());
        let account = StandardWalletAccount::new_with_key(key, Some(contract), settings(), None);
        let result = account.create_witness(b"sign-me");
        assert!(matches!(result, Err(WalletError::SigningFailed(_))));
    }
}
