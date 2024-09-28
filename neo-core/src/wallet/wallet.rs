use std::sync::Arc;
use crate::contract::Contract;
use crate::neo_contract::contract_parameters_context::ContractParametersContext;
use crate::payload::Version;
use crate::protocol_settings::ProtocolSettings;
use neo_type::H160;

/// The trait defining the interface for wallets.
pub trait Wallet {
    /// The error type returned when creating a wallet fails.
    type CreateError;

    /// Changes the password of the wallet.
    ///
    /// # Arguments
    ///
    /// * `old_password` - The old password of the wallet.
    /// * `new_password` - The new password to be used.
    ///
    /// # Returns
    ///
    /// `true` if the password is changed successfully; otherwise, `false`.
    fn change_password(&mut self, old_password: &str, new_password: &str) -> bool;

    /// Determines whether the specified account is included in the wallet.
    ///
    /// # Arguments
    ///
    /// * `script_hash` - The hash of the account.
    ///
    /// # Returns
    ///
    /// `true` if the account is included in the wallet; otherwise, `false`.
    fn contains(&self, script_hash: &H160) -> bool;

    /// Creates a standard account with the specified private key.
    ///
    /// # Arguments
    ///
    /// * `private_key` - The private key of the account.
    ///
    /// # Returns
    ///
    /// The created account.
    fn create_account(&mut self, private_key: &[u8]) -> WalletAccount;

    /// Creates a contract account for the wallet.
    ///
    /// # Arguments
    ///
    /// * `contract` - The contract of the account.
    /// * `key` - The private key of the account.
    ///
    /// # Returns
    ///
    /// The created account.
    fn create_contract_account(&mut self, contract: Contract, key: Option<KeyPair>) -> WalletAccount;

    /// Creates a watch-only account for the wallet.
    ///
    /// # Arguments
    ///
    /// * `script_hash` - The hash of the account.
    ///
    /// # Returns
    ///
    /// The created account.
    fn create_watch_only_account(&mut self, script_hash: H160) -> WalletAccount;

    /// Deletes the entire database of the wallet.
    fn delete(&mut self);

    /// Deletes an account from the wallet.
    ///
    /// # Arguments
    ///
    /// * `script_hash` - The hash of the account.
    ///
    /// # Returns
    ///
    /// `true` if the account is removed; otherwise, `false`.
    fn delete_account(&mut self, script_hash: &H160) -> bool;

    /// Gets the account with the specified hash.
    ///
    /// # Arguments
    ///
    /// * `script_hash` - The hash of the account.
    ///
    /// # Returns
    ///
    /// The account with the specified hash.
    fn get_account(&self, script_hash: &H160) -> Option<WalletAccount>;

    /// Gets all the accounts from the wallet.
    ///
    /// # Returns
    ///
    /// All accounts in the wallet.
    fn get_accounts(&self) -> Vec<WalletAccount>;

    /// Creates a standard account for the wallet.
    ///
    /// # Returns
    ///
    /// The created account.
    fn create_account_random(&mut self) -> WalletAccount;

    /// Creates a contract account for the wallet.
    ///
    /// # Arguments
    ///
    /// * `contract` - The contract of the account.
    /// * `private_key` - The private key of the account.
    ///
    /// # Returns
    ///
    /// The created account.
    fn create_contract_account_with_private_key(&mut self, contract: Contract, private_key: Option<&[u8]>) -> WalletAccount;

    /// Makes a transaction to transfer assets.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The snapshot used to read data.
    /// * `outputs` - The array of `TransferOutput` that contain the asset, amount, and targets of the transfer.
    /// * `from` - The account to transfer from.
    /// * `cosigners` - The cosigners to be added to the transaction.
    /// * `persisting_block` - The block environment to execute the transaction. If None, `ApplicationEngine::create_dummy_block` will be used.
    ///
    /// # Returns
    ///
    /// The created transaction.
    fn make_transaction(&self, snapshot: &Snapshot, outputs: &[TransferOutput], from: Option<&H160>, cosigners: Option<&[Signer]>, persisting_block: Option<&Block>) -> Result<Transaction, String>;

    /// Makes a transaction to run a smart contract.
    ///
    /// # Arguments
    ///
    /// * `snapshot` - The snapshot used to read data.
    /// * `script` - The script to be loaded in the transaction.
    /// * `sender` - The sender of the transaction.
    /// * `cosigners` - The cosigners to be added to the transaction.
    /// * `attributes` - The attributes to be added to the transaction.
    /// * `max_gas` - The maximum gas that can be spent to execute the script, in the unit of datoshi, 1 datoshi = 1e-8 GAS.
    /// * `persisting_block` - The block environment to execute the transaction. If None, `ApplicationEngine::create_dummy_block` will be used.
    ///
    /// # Returns
    ///
    /// The created transaction.
    fn make_transaction_with_script(&self, snapshot: &Snapshot, script: &[u8], sender: Option<&H160>, cosigners: Option<&[Signer]>, attributes: Option<&[TransactionAttribute]>, max_gas: i64, persisting_block: Option<&Block>) -> Result<Transaction, String>;

    /// Signs the `Verifiable` in the specified `ContractParametersContext` with the wallet.
    ///
    /// # Arguments
    ///
    /// * `context` - The `ContractParametersContext` to be used.
    ///
    /// # Returns
    ///
    /// `true` if the signature is successfully added to the context; otherwise, `false`.
    fn sign(&self, context: &mut ContractParametersContext) -> bool;

    /// Checks that the specified password is correct for the wallet.
    ///
    /// # Arguments
    ///
    /// * `password` - The password to be checked.
    ///
    /// # Returns
    ///
    /// `true` if the password is correct; otherwise, `false`.
    fn verify_password(&self, password: &str) -> bool;

    /// Saves the wallet file to the disk. It uses the value of `path` property.
    fn save(&self);

    /// Creates a new wallet.
    ///
    /// # Arguments
    ///
    /// * `name` - The name of the wallet.
    /// * `path` - The path where the wallet will be saved.
    /// * `password` - The password for the wallet.
    /// * `settings` - The protocol settings.
    ///
    /// # Returns
    ///
    /// The created wallet.
    fn create(name: &str, path: &str, password: &str, settings: Arc<ProtocolSettings>) -> Result<Self, Self::CreateError> where Self: Sized;

    /// Opens an existing wallet.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the wallet file.
    /// * `password` - The password of the wallet.
    /// * `settings` - The protocol settings.
    ///
    /// # Returns
    ///
    /// The opened wallet.
    fn open(path: &str, password: &str, settings: Arc<ProtocolSettings>) -> Result<Self, Self::CreateError> where Self: Sized;

    /// Migrates the accounts from old wallet to a new NEP6Wallet.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the new wallet file.
    /// * `old_path` - The path of the old wallet file.
    /// * `password` - The password of the wallets.
    /// * `settings` - The protocol settings to be used by the wallet.
    ///
    /// # Returns
    ///
    /// The created new wallet.
    fn migrate(path: &str, old_path: &str, password: &str, settings: Arc<ProtocolSettings>) -> Result<Self, Self::CreateError> where Self: Sized;
}

// Static methods can be implemented as associated functions in the trait

/// Decrypts a NEP-2 encrypted private key.
///
/// # Arguments
///
/// * `nep2` - The NEP-2 encrypted private key.
/// * `passphrase` - The passphrase used for encryption.
/// * `n` - The N parameter for Scrypt (iterations).
/// * `r` - The R parameter for Scrypt (block size).
/// * `p` - The P parameter for Scrypt (parallelization).
///
/// # Returns
///
/// The decrypted private key as a KeyPair, or an error if decryption fails.
pub fn get_private_key_from_nep2(nep2: &str, passphrase: &str, n: u32, r: u32, p: u32) -> Result<KeyPair, String> {
    // Implementation of NEP-2 decryption
    unimplemented!()
}

/// Imports a private key from WIF format.
///
/// # Arguments
///
/// * `wif` - The private key in WIF format.
///
/// # Returns
///
/// The imported private key as a KeyPair, or an error if the WIF is invalid.
pub fn get_private_key_from_wif(wif: &str) -> Result<KeyPair, String> {
    // Implementation of WIF import
    unimplemented!()
}

/// Verifies a message signature.
///
/// # Arguments
///
/// * `message` - The message that was signed.
/// * `signature` - The signature to verify.
/// * `pubkey` - The public key of the signer.
///
/// # Returns
///
/// `true` if the signature is valid, `false` otherwise.
pub fn verify_signature(message: &[u8], signature: &[u8], pubkey: &ECPoint) -> bool {
    // Implementation of signature verification
    unimplemented!()
}
