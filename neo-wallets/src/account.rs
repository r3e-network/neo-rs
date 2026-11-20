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
//! * [`Witness`] — re-exported from `neo_protocol::witness`, so wallet accounts build
//!   the same canonical witness payload the rest of the node verifies.
//!
//! # Signing adaptation
//!
//! `neo-core`'s `WalletAccount::create_witness` took a `&Transaction`. Because
//! transaction payloads live in forbidden crates, this port signs a
//! pre-computed *signature preimage* (`sign_data`) instead. Callers obtain that
//! preimage via `neo_protocol::get_sign_data_vec`, keeping the witness bytes
//! identical while removing the ledger/protocol-payload coupling.
//!
//! # Revocable account handles
//!
//! Wallets hand out cheap account handles (`Arc<dyn WalletAccount>`) that can
//! outlive the call which produced them. Two sharing rules keep locking a
//! wallet authoritative:
//!
//! * Key material lives in an `Arc<RwLock<Secret>>` shared by the wallet's
//!   canonical account entry and every handle, so locking the wallet drops the
//!   decrypted key everywhere at once instead of only in the wallet's copy.
//! * Every account also shares an [`UnlockState`] flag with its wallet. Secret
//!   revealing operations ([`WalletAccount::get_key`],
//!   [`WalletAccount::export_wif`], [`WalletAccount::export_nep2`] and
//!   [`WalletAccount::create_witness`]) fail closed as soon as the wallet clears
//!   that flag, which covers key material the wallet cannot drop (an account
//!   created from a raw private key has no NEP-2 payload to fall back on).

use crate::helper::Helper;
use crate::key_pair::KeyPair;
use crate::signing::witness_from_signature;
use crate::wallet::{WalletError, WalletResult};
use async_trait::async_trait;
use neo_primitives::UInt160;
use neo_protocol::ProtocolSettings;
use neo_smart_contract::helper::Helper as ContractHelper;
use parking_lot::RwLock;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

// Re-export types from their canonical locations
pub use neo_protocol::witness::Witness;
pub use neo_smart_contract::contract::Contract;

/// `PUSHDATA1` opcode byte (matches `neo_vm::OpCode::PUSHDATA1`).
#[cfg(test)]
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

/// Wallet-wide unlock flag shared between a wallet and every account handle it
/// has handed out.
///
/// An account handle is a cheap clone that keeps its own copy of the account,
/// so clearing the wallet's canonical entries on lock is *not* enough to revoke
/// access through handles obtained earlier. Wallets therefore share this flag
/// with their accounts and every secret-revealing operation fails closed once
/// the flag is cleared.
#[derive(Debug)]
pub struct UnlockState {
    unlocked: AtomicBool,
}

impl UnlockState {
    /// Creates a state with the supplied initial unlock flag.
    #[must_use]
    pub const fn new(unlocked: bool) -> Self {
        Self {
            unlocked: AtomicBool::new(unlocked),
        }
    }

    /// Returns whether secret access is currently permitted.
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.unlocked.load(Ordering::Acquire)
    }

    /// Sets the unlock flag, granting or revoking secret access.
    pub fn set_unlocked(&self, unlocked: bool) {
        self.unlocked.store(unlocked, Ordering::Release);
    }
}

impl Default for UnlockState {
    fn default() -> Self {
        // A stand-alone account manages its own lock state; only wallets wire
        // in a flag that can revoke access behind the account's back.
        Self::new(true)
    }
}

/// Decrypted key material plus its NEP-2 payload for a single account.
///
/// Shared (`Arc<RwLock<..>>`) between the wallet's canonical account entry and
/// every handle handed out for it, so locking the wallet physically drops the
/// decrypted key in all copies.
#[derive(Debug, Default)]
struct Secret {
    key_pair: Option<KeyPair>,
    nep2_key: Option<String>,
    is_locked: bool,
}

/// Concrete wallet account implementation providing the same ergonomics as the
/// C# `WalletAccount` class.
#[derive(Debug, Clone)]
pub struct StandardWalletAccount {
    script_hash: UInt160,
    label: Option<String>,
    is_default: bool,
    contract: Option<Contract>,
    /// Key material shared with every clone of this account.
    secrets: Arc<RwLock<Secret>>,
    /// Unlock flag shared with the owning wallet.
    unlock_state: Arc<UnlockState>,
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
            contract,
            secrets: Arc::new(RwLock::new(Secret {
                key_pair: Some(key_pair),
                nep2_key,
                is_locked: false,
            })),
            unlock_state: Arc::new(UnlockState::default()),
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
            contract,
            secrets: Arc::new(RwLock::new(Secret::default())),
            unlock_state: Arc::new(UnlockState::default()),
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
            contract,
            secrets: Arc::new(RwLock::new(Secret {
                key_pair: None,
                nep2_key: Some(nep2_key),
                is_locked: true,
            })),
            unlock_state: Arc::new(UnlockState::default()),
            protocol_settings,
        }
    }

    /// Returns the stored NEP-2 key (if any).
    #[must_use]
    pub fn nep2_key(&self) -> Option<String> {
        self.secrets.read().nep2_key.clone()
    }

    /// Replaces the stored NEP-2 payload.
    pub fn set_nep2_key(&mut self, nep2: Option<String>) {
        self.secrets.write().nep2_key = nep2;
    }

    /// Attaches the wallet-wide unlock flag that gates secret access.
    ///
    /// Wallets call this for every account they store so that a later lock also
    /// revokes access through handles that were already handed out.
    pub fn set_unlock_state(&mut self, state: Arc<UnlockState>) {
        self.unlock_state = state;
    }

    /// Returns the unlock flag shared with the owning wallet.
    #[must_use]
    pub fn unlock_state(&self) -> &Arc<UnlockState> {
        &self.unlock_state
    }

    /// Returns the decrypted key pair regardless of the wallet unlock flag.
    ///
    /// Only used to persist key material: the value is handed straight to NEP-2
    /// encryption and never leaves the crate.
    pub(crate) fn key_for_persistence(&self) -> Option<KeyPair> {
        self.secrets.read().key_pair.clone()
    }

    /// Fails closed when the owning wallet has been locked.
    fn ensure_unlocked(&self) -> WalletResult<()> {
        if self.unlock_state.is_unlocked() {
            Ok(())
        } else {
            Err(WalletError::WalletLocked)
        }
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
        self.secrets.read().is_locked || !self.unlock_state.is_unlocked()
    }

    fn has_key(&self) -> bool {
        let secret = self.secrets.read();
        secret.key_pair.is_some() || secret.nep2_key.is_some()
    }

    fn get_key(&self) -> Option<KeyPair> {
        // Handles outlive the wallet call that produced them: report no key once
        // the wallet is locked even though this copy may still hold one.
        self.ensure_unlocked().ok()?;
        self.secrets.read().key_pair.clone()
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
        let nep2 = {
            let secret = self.secrets.read();
            if !secret.is_locked {
                return Ok(true);
            }
            secret.nep2_key.clone()
        };

        let Some(nep2) = nep2 else {
            return Ok(false);
        };

        let version = self.protocol_settings.address_version;
        match KeyPair::from_nep2_string(&nep2, password, version) {
            Ok(key_pair) => {
                let mut secret = self.secrets.write();
                secret.key_pair = Some(key_pair);
                secret.is_locked = false;
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    fn lock(&mut self) {
        let mut secret = self.secrets.write();
        // Only accounts with an encrypted payload can be re-opened; dropping a
        // transient raw key here would destroy the only copy of it.
        if secret.nep2_key.is_some() {
            secret.key_pair = None;
            secret.is_locked = true;
        }
    }

    fn verify_password(&self, password: &str) -> WalletResult<bool> {
        let (nep2, has_key) = {
            let secret = self.secrets.read();
            (secret.nep2_key.clone(), secret.key_pair.is_some())
        };
        match nep2 {
            Some(nep2) => {
                let version = self.protocol_settings.address_version;
                Ok(KeyPair::from_nep2_string(&nep2, password, version).is_ok())
            }
            None => Ok(has_key),
        }
    }

    fn export_wif(&self) -> WalletResult<String> {
        self.ensure_unlocked()?;
        let secret = self.secrets.read();
        match &secret.key_pair {
            Some(key) => Ok(key.to_wif()),
            None => Err(WalletError::AccountLocked),
        }
    }

    fn export_nep2(&self, password: &str) -> WalletResult<String> {
        {
            let secret = self.secrets.read();
            // The encrypted payload is safe to hand out while locked.
            if let Some(nep2) = &secret.nep2_key {
                return Ok(nep2.clone());
            }
        }

        self.ensure_unlocked()?;
        let secret = self.secrets.read();
        let key = secret.key_pair.as_ref().ok_or(WalletError::AccountLocked)?;
        let version = self.protocol_settings.address_version;
        key.to_nep2(password, version)
    }

    fn create_witness(&self, sign_data: &[u8]) -> WalletResult<Witness> {
        self.ensure_unlocked()?;
        let secret = self.secrets.read();
        let key = secret.key_pair.as_ref().ok_or(WalletError::AccountLocked)?;

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

        witness_from_signature(&signature, &verification_script)
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
        assert_eq!(witness.verification_script(), expected_script);
        // invocation = PUSHDATA1 <64> <signature>
        assert_eq!(witness.invocation_script()[0], OP_PUSHDATA1);
        assert_eq!(witness.invocation_script()[1], 64);
        assert_eq!(witness.invocation_script().len(), 66);
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
        assert_eq!(witness.invocation_script().len(), 66);
        assert_eq!(witness.invocation_script()[0], OP_PUSHDATA1);
        assert_eq!(witness.invocation_script()[1], 64);
        assert_eq!(witness.verification_script().len(), 40);
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
