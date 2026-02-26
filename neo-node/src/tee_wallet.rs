//! TEE-backed wallet implementation for RPC and local signing.

use crate::tee_integration::TeeRuntime;
use neo_core::cryptography::{ECCurve, ECPoint};
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::ContractParameterType;
use neo_core::wallets::wallet_account::WalletAccount;
use neo_core::wallets::{KeyPair, Version, Wallet, WalletError, WalletResult};
use neo_core::{UInt160, UInt256};
use neo_tee::{SealedKey, TeeError, TeeWallet as EnclaveWallet};
use neo_vm::op_code::OpCode;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct TeeWalletAdapter {
    name: String,
    wallet_path: PathBuf,
    wallet: Arc<EnclaveWallet>,
    protocol_settings: Arc<ProtocolSettings>,
    version: Version,
    accounts: Arc<RwLock<HashMap<UInt160, Arc<TeeWalletAccount>>>>,
    default_account: RwLock<Option<UInt160>>,
}

impl TeeWalletAdapter {
    pub fn from_runtime(
        runtime: Arc<TeeRuntime>,
        settings: Arc<ProtocolSettings>,
        wallet_path: &Path,
    ) -> WalletResult<Self> {
        let wallet = Arc::new(open_or_create_wallet(&runtime, wallet_path)?);

        if wallet.list_keys().is_empty() {
            wallet
                .create_key(Some("default".to_string()))
                .map_err(map_tee_error)?;
        }

        let mut keys = wallet.list_keys();
        if keys.is_empty() {
            return Err(WalletError::Other(
                "TEE wallet did not expose any keys after initialization".to_string(),
            ));
        }

        let default_hash_bytes = if let Some(key) = wallet.default_account() {
            key.script_hash
        } else {
            let first = keys[0].script_hash;
            wallet.set_default_account(&first).map_err(map_tee_error)?;
            first
        };

        let default_hash = script_hash_to_uint160(&default_hash_bytes)?;
        let mut accounts = HashMap::with_capacity(keys.len());
        for key in keys.drain(..) {
            let account_hash = script_hash_to_uint160(&key.script_hash)?;
            let is_default = account_hash == default_hash;
            let account = Arc::new(TeeWalletAccount::from_sealed_key(
                Arc::clone(&wallet),
                Arc::clone(&settings),
                key,
                is_default,
            )?);
            accounts.insert(account_hash, account);
        }

        let wallet_name = wallet_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("tee-wallet")
            .to_string();

        Ok(Self {
            name: wallet_name,
            wallet_path: wallet_path.to_path_buf(),
            wallet,
            protocol_settings: settings,
            version: Version::new(1, 0, 0),
            accounts: Arc::new(RwLock::new(accounts)),
            default_account: RwLock::new(Some(default_hash)),
        })
    }

    fn account_for_hash(&self, script_hash: &UInt160) -> WalletResult<Arc<TeeWalletAccount>> {
        self.accounts
            .read()
            .get(script_hash)
            .cloned()
            .ok_or(WalletError::AccountNotFound(*script_hash))
    }

    fn update_default_account(&self, new_default: Option<UInt160>) {
        *self.default_account.write() = new_default;
        let accounts = self.accounts.read();
        for (hash, account) in accounts.iter() {
            account.set_default_flag(new_default.is_some_and(|default| default == *hash));
        }
    }

    fn insert_or_update_account(
        &self,
        sealed_key: SealedKey,
        is_default: bool,
    ) -> WalletResult<Arc<TeeWalletAccount>> {
        let account_hash = script_hash_to_uint160(&sealed_key.script_hash)?;
        let account = Arc::new(TeeWalletAccount::from_sealed_key(
            Arc::clone(&self.wallet),
            Arc::clone(&self.protocol_settings),
            sealed_key,
            is_default,
        )?);
        self.accounts
            .write()
            .insert(account_hash, Arc::clone(&account));
        if is_default {
            self.update_default_account(Some(account_hash));
        }
        Ok(account)
    }
}

#[async_trait::async_trait]
impl Wallet for TeeWalletAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> Option<&str> {
        self.wallet_path.to_str()
    }

    fn version(&self) -> &Version {
        &self.version
    }

    async fn change_password(
        &self,
        _old_password: &str,
        _new_password: &str,
    ) -> WalletResult<bool> {
        Err(WalletError::Other(
            "TEE wallets do not use password-based key encryption".to_string(),
        ))
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        self.accounts.read().contains_key(script_hash)
    }

    async fn create_account(&self, private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        let sealed = self
            .wallet
            .import_key(private_key, None)
            .map_err(map_tee_error)?;
        let is_default = self.default_account.read().is_none();
        if is_default {
            self.wallet
                .set_default_account(&sealed.script_hash)
                .map_err(map_tee_error)?;
        }

        let account = self.insert_or_update_account(sealed, is_default)?;
        Ok(account as Arc<dyn WalletAccount>)
    }

    async fn create_account_with_contract(
        &self,
        contract: Contract,
        key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        let Some(key_pair) = key_pair else {
            return Err(WalletError::Other(
                "TEE wallets do not support watch-only contract accounts".to_string(),
            ));
        };

        let account = self.create_account(key_pair.private_key()).await?;
        if account.script_hash() != contract.script_hash() {
            return Err(WalletError::Other(
                "Provided contract does not match imported private key".to_string(),
            ));
        }
        Ok(account)
    }

    async fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "TEE wallets do not support watch-only accounts".to_string(),
        ))
    }

    async fn delete_account(&self, script_hash: &UInt160) -> WalletResult<bool> {
        if !self.contains(script_hash) {
            return Err(WalletError::AccountNotFound(*script_hash));
        }

        let script_hash_bytes = uint160_to_script_hash_bytes(script_hash);
        self.wallet
            .delete_key(&script_hash_bytes)
            .map_err(map_tee_error)?;
        self.accounts.write().remove(script_hash);

        let current_default = *self.default_account.read();
        if current_default == Some(*script_hash) {
            let next_default = self.accounts.read().keys().next().copied();
            if let Some(hash) = next_default {
                let hash_bytes = uint160_to_script_hash_bytes(&hash);
                self.wallet
                    .set_default_account(&hash_bytes)
                    .map_err(map_tee_error)?;
                self.update_default_account(Some(hash));
            } else {
                self.update_default_account(None);
            }
        }

        Ok(true)
    }

    async fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(WalletError::Other(
            "TEE wallets cannot export private keys".to_string(),
        ))
    }

    fn get_account(&self, script_hash: &UInt160) -> Option<Arc<dyn WalletAccount>> {
        self.accounts
            .read()
            .get(script_hash)
            .cloned()
            .map(|account| account as Arc<dyn WalletAccount>)
    }

    fn get_accounts(&self) -> Vec<Arc<dyn WalletAccount>> {
        self.accounts
            .read()
            .values()
            .cloned()
            .map(|account| account as Arc<dyn WalletAccount>)
            .collect()
    }

    async fn get_available_balance(&self, _asset_id: &UInt256) -> WalletResult<i64> {
        Err(WalletError::Other(
            "TEE wallets do not track balances".to_string(),
        ))
    }

    async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
        Err(WalletError::Other(
            "TEE wallets do not track unclaimed GAS".to_string(),
        ))
    }

    async fn import_wif(&self, wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        let key_pair = KeyPair::from_wif(wif).map_err(|e| WalletError::Other(e.to_string()))?;
        self.create_account(key_pair.private_key()).await
    }

    async fn import_nep2(
        &self,
        nep2_key: &str,
        password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        let key_pair =
            KeyPair::from_nep2_string(nep2_key, password, self.protocol_settings.address_version)
                .map_err(|e| WalletError::Other(e.to_string()))?;
        self.create_account(key_pair.private_key()).await
    }

    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        let account = self.account_for_hash(script_hash)?;
        account.sign_data(data)
    }

    async fn sign_transaction(&self, transaction: &mut Transaction) -> WalletResult<()> {
        let signer_hashes: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();

        for hash in signer_hashes {
            if let Ok(account) = self.account_for_hash(&hash) {
                let witness = account.sign_transaction(transaction)?;
                transaction.add_witness(witness);
            }
        }

        Ok(())
    }

    async fn unlock(&self, _password: &str) -> WalletResult<bool> {
        self.wallet.unlock().map_err(map_tee_error)?;
        Ok(true)
    }

    fn lock(&self) {
        self.wallet.lock();
    }

    async fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    async fn save(&self) -> WalletResult<()> {
        Ok(())
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        let hash = (*self.default_account.read())?;
        self.accounts
            .read()
            .get(&hash)
            .cloned()
            .map(|account| account as Arc<dyn WalletAccount>)
    }

    async fn set_default_account(&self, script_hash: &UInt160) -> WalletResult<()> {
        if !self.accounts.read().contains_key(script_hash) {
            return Err(WalletError::AccountNotFound(*script_hash));
        }

        let hash_bytes = uint160_to_script_hash_bytes(script_hash);
        self.wallet
            .set_default_account(&hash_bytes)
            .map_err(map_tee_error)?;
        self.update_default_account(Some(*script_hash));
        Ok(())
    }
}

pub struct TeeWalletAccount {
    script_hash: UInt160,
    script_hash_bytes: [u8; 20],
    label: Option<String>,
    is_default: AtomicBool,
    contract: Contract,
    wallet: Arc<EnclaveWallet>,
    protocol_settings: Arc<ProtocolSettings>,
}

impl TeeWalletAccount {
    fn from_sealed_key(
        wallet: Arc<EnclaveWallet>,
        protocol_settings: Arc<ProtocolSettings>,
        sealed_key: SealedKey,
        is_default: bool,
    ) -> WalletResult<Self> {
        let script_hash = script_hash_to_uint160(&sealed_key.script_hash)?;
        let contract = signature_contract_from_public_key(&sealed_key.public_key)?;

        if contract.script_hash() != script_hash {
            return Err(WalletError::Other(format!(
                "Script hash mismatch for TEE key {}",
                script_hash
            )));
        }

        Ok(Self {
            script_hash,
            script_hash_bytes: sealed_key.script_hash,
            label: sealed_key.label,
            is_default: AtomicBool::new(is_default),
            contract,
            wallet,
            protocol_settings,
        })
    }

    fn set_default_flag(&self, is_default: bool) {
        self.is_default.store(is_default, Ordering::SeqCst);
    }

    fn sign_data(&self, data: &[u8]) -> WalletResult<Vec<u8>> {
        let signature = self
            .wallet
            .sign(&self.script_hash_bytes, data)
            .map_err(map_tee_error)?;

        if signature.len() != 64 {
            return Err(WalletError::SigningFailed(
                "Invalid TEE signature length".to_string(),
            ));
        }

        Ok(signature)
    }

    fn sign_transaction(&self, transaction: &Transaction) -> WalletResult<Witness> {
        let sign_data = get_sign_data_vec(transaction, self.protocol_settings.network)
            .map_err(|err| WalletError::SigningFailed(err.to_string()))?;
        let signature = self.sign_data(&sign_data)?;
        let invocation = signature_invocation(&signature)?;
        Ok(Witness::new_with_scripts(
            invocation,
            self.contract.script.clone(),
        ))
    }
}

impl WalletAccount for TeeWalletAccount {
    fn script_hash(&self) -> UInt160 {
        self.script_hash
    }

    fn address(&self) -> String {
        neo_core::wallets::helper::Helper::to_address(
            &self.script_hash,
            self.protocol_settings.address_version,
        )
    }

    fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    fn set_label(&mut self, label: Option<String>) {
        self.label = label;
    }

    fn is_default(&self) -> bool {
        self.is_default.load(Ordering::SeqCst)
    }

    fn set_is_default(&mut self, is_default: bool) {
        self.is_default.store(is_default, Ordering::SeqCst);
    }

    fn is_locked(&self) -> bool {
        self.wallet.is_locked()
    }

    fn has_key(&self) -> bool {
        true
    }

    fn get_key(&self) -> Option<KeyPair> {
        None
    }

    fn contract(&self) -> Option<&Contract> {
        Some(&self.contract)
    }

    fn set_contract(&mut self, contract: Option<Contract>) {
        if let Some(contract) = contract {
            self.script_hash = contract.script_hash();
            self.script_hash_bytes = uint160_to_script_hash_bytes(&self.script_hash);
            self.contract = contract;
        }
    }

    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        &self.protocol_settings
    }

    fn unlock(&mut self, _password: &str) -> WalletResult<bool> {
        self.wallet.unlock().map_err(map_tee_error)?;
        Ok(true)
    }

    fn lock(&mut self) {
        self.wallet.lock();
    }

    fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(true)
    }

    fn export_wif(&self) -> WalletResult<String> {
        Err(WalletError::Other(
            "TEE accounts cannot export WIF".to_string(),
        ))
    }

    fn export_nep2(&self, _password: &str) -> WalletResult<String> {
        Err(WalletError::Other(
            "TEE accounts cannot export NEP-2".to_string(),
        ))
    }

    fn create_witness(&self, transaction: &Transaction) -> WalletResult<Witness> {
        let sign_data = get_sign_data_vec(transaction, self.protocol_settings.network)
            .map_err(|err| WalletError::SigningFailed(err.to_string()))?;
        let signature = self.sign_data(&sign_data)?;
        let invocation = signature_invocation(&signature)?;
        Ok(Witness::new_with_scripts(
            invocation,
            self.contract.script.clone(),
        ))
    }
}

fn open_or_create_wallet(runtime: &TeeRuntime, wallet_path: &Path) -> WalletResult<EnclaveWallet> {
    if wallet_path.exists() {
        if wallet_path.is_file() {
            return Err(WalletError::Other(format!(
                "TEE wallet path must be a directory, got file: {}",
                wallet_path.display()
            )));
        }
        if neo_tee::TeeWalletProvider::is_tee_wallet(wallet_path) {
            return runtime
                .wallet_provider
                .open_wallet(wallet_path)
                .map_err(map_tee_error);
        }

        let is_empty_dir = wallet_path
            .read_dir()
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !is_empty_dir {
            return Err(WalletError::Other(format!(
                "directory is not a TEE wallet: {}",
                wallet_path.display()
            )));
        }
    }

    let name = wallet_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tee-wallet");
    runtime
        .wallet_provider
        .create_wallet(name, wallet_path)
        .map_err(map_tee_error)
}

fn script_hash_to_uint160(script_hash: &[u8; 20]) -> WalletResult<UInt160> {
    UInt160::from_bytes(script_hash).map_err(|err| WalletError::Other(err.to_string()))
}

fn uint160_to_script_hash_bytes(script_hash: &UInt160) -> [u8; 20] {
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(script_hash.as_bytes().as_ref());
    bytes
}

fn signature_contract_from_public_key(public_key: &[u8]) -> WalletResult<Contract> {
    let point = ECPoint::new(ECCurve::Secp256r1, public_key.to_vec())
        .map_err(|err| WalletError::SigningFailed(err.to_string()))?;
    Ok(Contract::create(
        vec![ContractParameterType::Signature],
        Contract::create_signature_redeem_script(point),
    ))
}

fn signature_invocation(signature: &[u8]) -> WalletResult<Vec<u8>> {
    if signature.len() != 64 {
        return Err(WalletError::SigningFailed(
            "Signature must be 64 bytes".to_string(),
        ));
    }

    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1 as u8);
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(signature);
    Ok(invocation)
}

fn map_tee_error(err: TeeError) -> WalletError {
    match err {
        TeeError::Other(message) if message.contains("Wallet is locked") => {
            WalletError::AccountLocked
        }
        other => WalletError::Other(other.to_string()),
    }
}

#[cfg(all(test, not(feature = "tee-sgx")))]
mod tests {
    use super::*;
    use neo_core::network::p2p::payloads::signer::Signer;
    use neo_core::smart_contract::helper::Helper as ContractHelper;
    use neo_core::WitnessScope;
    use neo_crypto::Secp256r1Crypto;
    use tempfile::tempdir;

    #[tokio::test]
    async fn tee_wallet_signs_and_builds_witness() {
        let temp = tempdir().expect("temp dir");
        let runtime = Arc::new(
            TeeRuntime::new(temp.path().join("tee-runtime"), "batched", 1000).expect("tee runtime"),
        );

        let settings = Arc::new(ProtocolSettings::default());
        let wallet_path = temp.path().join("wallet");
        let wallet = TeeWalletAdapter::from_runtime(
            Arc::clone(&runtime),
            Arc::clone(&settings),
            &wallet_path,
        )
        .expect("tee wallet adapter");

        let default_account = wallet.get_default_account().expect("default account");
        let script_hash = default_account.script_hash();

        let payload = b"neo-tee-wallet";
        let signature = wallet.sign(payload, &script_hash).await.expect("sign data");
        assert_eq!(signature.len(), 64);

        let tee_wallet = runtime
            .wallet_provider
            .open_wallet(&wallet_path)
            .expect("open tee wallet");
        let key = tee_wallet.default_account().expect("default tee key");
        let signature_bytes: [u8; 64] = signature.as_slice().try_into().expect("sig bytes");
        assert!(
            Secp256r1Crypto::verify(payload, &signature_bytes, &key.public_key).expect("verify")
        );

        let mut tx = Transaction::new();
        tx.set_script(vec![OpCode::PUSH1 as u8]);
        tx.set_valid_until_block(1);
        tx.add_signer(Signer::new(script_hash, WitnessScope::CALLED_BY_ENTRY));
        wallet.sign_transaction(&mut tx).await.expect("sign tx");

        assert_eq!(tx.witnesses().len(), 1);
        let witness = &tx.witnesses()[0];
        let expected_verification = ContractHelper::signature_redeem_script(&key.public_key);
        assert_eq!(witness.verification_script, expected_verification);

        assert_eq!(witness.invocation_script[0], OpCode::PUSHDATA1 as u8);
        let sig_len = witness.invocation_script[1] as usize;
        assert_eq!(sig_len, 64);
        let sig_slice = &witness.invocation_script[2..2 + sig_len];
        let sig_bytes: [u8; 64] = sig_slice.try_into().expect("sig slice");
        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        assert!(
            Secp256r1Crypto::verify(&sign_data, &sig_bytes, &key.public_key)
                .expect("verify tx signature")
        );
    }
}
