use crate::contract::Contract;
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;
use crate::wallet::key_pair::KeyPair;

/// Trait representing the common functionality of a wallet account.
pub trait WalletAccountTrait {
    /// Gets the address of the account.
    fn address(&self) -> String;

    /// Indicates whether the account contains a private key.
    fn has_key(&self) -> bool;

    /// Indicates whether the account is a watch-only account.
    fn watch_only(&self) -> bool;

    /// Gets the private key of the account.
    fn get_key(&self) -> Option<KeyPair>;

    /// Gets the protocol settings of the account.
    fn get_protocol_settings(&self) -> &ProtocolSettings;

    /// Sets the protocol settings of the account.
    fn set_protocol_settings(&mut self, settings: ProtocolSettings);

    /// Gets the script hash of the account.
    fn get_script_hash(&self) -> &H160;

    /// Sets the script hash of the account.
    fn set_script_hash(&mut self, script_hash: H160);

    /// Gets the label of the account.
    fn get_label(&self) -> &str;

    /// Sets the label of the account.
    fn set_label(&mut self, label: String);

    /// Checks if the account is the default account.
    fn is_default(&self) -> bool;

    /// Sets whether the account is the default account.
    fn set_default(&mut self, is_default: bool);

    /// Checks if the account is locked.
    fn is_locked(&self) -> bool;

    /// Sets whether the account is locked.
    fn set_locked(&mut self, locked: bool);

    /// Gets the contract of the account.
    fn get_contract(&self) -> Option<&Contract>;

    /// Sets the contract of the account.
    fn set_contract(&mut self, contract: Option<Contract>);
}

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
}

impl WalletAccountTrait for WalletAccount {
    fn address(&self) -> String {
        self.script_hash.to_address(self.protocol_settings.address_version)
    }

    fn has_key(&self) -> bool {
        // This should be implemented by derived structs
        unimplemented!("has_key() must be implemented by derived structs")
    }

    fn watch_only(&self) -> bool {
        self.contract.is_none()
    }

    fn get_key(&self) -> Option<KeyPair> {
        // This should be implemented by derived structs
        unimplemented!("get_key() must be implemented by derived structs")
    }
    
    fn get_protocol_settings(&self) -> &ProtocolSettings {
        &self.protocol_settings
    }
    
    fn set_protocol_settings(&mut self, settings: ProtocolSettings) {
        self.protocol_settings = settings;
    }
    
    fn get_script_hash(&self) -> &H160 {
        &self.script_hash
    }
    
    fn set_script_hash(&mut self, script_hash: H160) {
        self.script_hash = script_hash;
    }
    
    fn get_label(&self) -> &str {
        &self.label
    }
    
    fn set_label(&mut self, label: String) {
        self.label = label;
    }
    
    fn is_default(&self) -> bool {
        self.is_default
    }
    
    fn set_default(&mut self, is_default: bool) {
        self.is_default = is_default;
    }
    
    fn is_locked(&self) -> bool {
        self.lock
    }
    
    fn set_locked(&mut self, locked: bool) {
        self.lock = locked;
    }
    
    fn get_contract(&self) -> Option<&Contract> {
        self.contract.as_ref()
    }
    
    fn set_contract(&mut self, contract: Option<Contract>) {
        self.contract = contract;
    }
}
