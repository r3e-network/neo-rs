use crate::contract::Contract;
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;
use crate::wallet::KeyPair;

/// Represents an account in a wallet.
pub struct WalletAccount {
    /// The `ProtocolSettings` to be used by the wallet.
    protocol_settings: ProtocolSettings,

    /// The hash of the account.
    pub script_hash: H160,

    /// The label of the account.
    pub label: String,

    /// Indicates whether the account is the default account in the wallet.
    pub is_default: bool,

    /// Indicates whether the account is locked.
    pub lock: bool,

    /// The contract of the account.
    pub contract: Option<Contract>,
}

impl WalletAccount {
    /// Creates a new instance of the `WalletAccount` struct.
    ///
    /// # Arguments
    ///
    /// * `script_hash` - The hash of the account.
    /// * `settings` - The `ProtocolSettings` to be used by the wallet.
    pub fn new(script_hash: H160, settings: ProtocolSettings) -> Self {
        Self {
            protocol_settings: settings,
            script_hash,
            label: String::new(),
            is_default: false,
            lock: false,
            contract: None,
        }
    }

    /// Gets the address of the account.
    pub fn address(&self) -> String {
        self.script_hash.to_address(self.protocol_settings.address_version)
    }

    /// Indicates whether the account contains a private key.
    pub fn has_key(&self) -> bool {
        // This should be implemented by derived structs
        unimplemented!("has_key() must be implemented by derived structs")
    }

    /// Indicates whether the account is a watch-only account.
    pub fn watch_only(&self) -> bool {
        self.contract.is_none()
    }

    /// Gets the private key of the account.
    ///
    /// # Returns
    ///
    /// The private key of the account, or `None` if there is no private key in the account.
    pub fn get_key(&self) -> Option<KeyPair> {
        // This should be implemented by derived structs
        unimplemented!("get_key() must be implemented by derived structs")
    }
}
