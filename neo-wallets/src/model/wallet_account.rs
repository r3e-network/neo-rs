//! Wallet account abstraction and standard implementation.
//!
//! This module mirrors the behaviour of the C# `Neo.Wallets.WalletAccount`
//! hierarchy, providing the core primitives used by NEP-6 wallets and other
//! account containers within the runtime.

use crate::key_pair::KeyPair;
use crate::wallet::{WalletError, WalletResult};
use crate::wallet_helper::WalletAddress;
use neo_config::ProtocolSettings;
use neo_execution::Contract;
use neo_payloads::Transaction;
use neo_payloads::Witness;
use neo_primitives::UInt160;
use std::sync::Arc;

/// Common interface shared by all wallet-backed accounts.
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
    fn unlock(&mut self, password: &str) -> WalletResult<bool>;

    /// Locks the account (when a NEP-2 key is available).
    fn lock(&mut self);

    /// Validates that the supplied password can decrypt the encrypted key.
    fn verify_password(&self, password: &str) -> WalletResult<bool>;

    /// Exports the key material in WIF format.
    fn export_wif(&self) -> WalletResult<String>;

    /// Exports the key material encoded as NEP-2.
    fn export_nep2(&self, password: &str) -> WalletResult<String>;

    /// Creates a witness for the supplied transaction using the account credentials.
    fn create_witness(&self, transaction: &Transaction) -> WalletResult<Witness>;
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
    /// NEP-2 scrypt cost parameters `(N, r, p)`. A NEP-6 wallet carries its own
    /// `ScryptParameters`, so the owning wallet sets these; they default to the
    /// NEP-6 standard (16384, 8, 8). C# `NEP6Account` decrypts with
    /// `wallet.Scrypt.N/R/P`, so a non-default wallet only decrypts with matching
    /// values.
    scrypt: (u32, u32, u32),
}

/// NEP-6 default scrypt parameters `(N, r, p)`.
const DEFAULT_SCRYPT: (u32, u32, u32) = (16384, 8, 8);

impl StandardWalletAccount {
    /// Creates an account backed by the provided key pair.
    pub fn new_with_key(
        key_pair: KeyPair,
        contract: Option<Contract>,
        protocol_settings: Arc<ProtocolSettings>,
        nep2_key: Option<String>,
    ) -> Self {
        let script_hash = contract
            .as_ref()
            .map(|contract| contract.script_hash())
            .unwrap_or_else(|| key_pair.get_script_hash());

        Self {
            script_hash,
            label: None,
            is_default: false,
            is_locked: false,
            contract,
            key_pair: Some(key_pair),
            nep2_key,
            protocol_settings,
            scrypt: DEFAULT_SCRYPT,
        }
    }

    /// Creates a watch-only account for the supplied script hash.
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
            scrypt: DEFAULT_SCRYPT,
        }
    }

    /// Creates an account backed solely by a NEP-2 encrypted key.
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
            scrypt: DEFAULT_SCRYPT,
        }
    }

    /// Returns the stored NEP-2 key (if any).
    pub fn nep2_key(&self) -> Option<&str> {
        self.nep2_key.as_deref()
    }

    /// Replaces the stored NEP-2 payload.
    pub fn set_nep2_key(&mut self, nep2: Option<String>) {
        self.nep2_key = nep2;
    }

    /// Sets the NEP-2 scrypt cost parameters `(N, r, p)` this account's key was
    /// encrypted with. A NEP-6 wallet calls this with its own `ScryptParameters`
    /// so a wallet created with non-default parameters decrypts correctly.
    pub fn set_scrypt_parameters(&mut self, n: u32, r: u32, p: u32) {
        self.scrypt = (n, r, p);
    }
}

impl WalletAccount for StandardWalletAccount {
    fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    fn address(&self) -> String {
        WalletAddress::to_address(&self.script_hash, self.protocol_settings.address_version)
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

        let nep2 = match &self.nep2_key {
            Some(value) => value,
            None => return Ok(false),
        };

        let version = self.protocol_settings.address_version;
        let (n, r, p) = self.scrypt;
        match KeyPair::from_nep2_string_with_params(nep2, password, version, n, r, p) {
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
            let (n, r, p) = self.scrypt;
            Ok(KeyPair::from_nep2_string_with_params(nep2, password, version, n, r, p).is_ok())
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
        let (n, r, p) = self.scrypt;
        key.to_nep2_with_params(password, version, n, r, p)
            .map_err(|e| WalletError::SigningFailed(e.to_string()))
    }

    fn create_witness(&self, transaction: &Transaction) -> WalletResult<Witness> {
        let key = self.key_pair.as_ref().ok_or(WalletError::AccountLocked)?;
        let sign_data =
            neo_payloads::get_sign_data_vec(transaction, self.protocol_settings.network)
                .map_err(|e| WalletError::SigningFailed(e.to_string()))?;
        let signature = key
            .sign(&sign_data)
            .map_err(|e| WalletError::SigningFailed(e.to_string()))?;

        let verification_script = if let Some(contract) = &self.contract {
            contract.script.clone()
        } else {
            key.get_verification_script()
        };

        // Use the shared helper, which validates the signature is exactly 64
        // bytes before emitting the PUSHDATA1 length byte (avoiding a silent
        // `len as u8` truncation) and matches the C# wallet signing path.
        let invocation = crate::signature_invocation(&signature)?;

        Ok(Witness::new_with_scripts(invocation, verification_script))
    }
}
