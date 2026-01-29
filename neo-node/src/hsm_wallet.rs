//! HSM-backed wallet implementation for RPC signing.

use anyhow::{anyhow, Result};
use neo_core::cryptography::{ECCurve, ECPoint};
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::ContractParameterType;
use neo_core::wallets::wallet_account::WalletAccount;
use neo_core::wallets::{Version, Wallet, WalletError, WalletResult};
use neo_core::{UInt160, UInt256};
use neo_hsm::{HsmKeyInfo, HsmSigner};
use neo_vm::op_code::OpCode;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use crate::hsm_integration::HsmRuntime;

pub struct HsmWallet {
    name: String,
    signer: Arc<dyn HsmSigner>,
    protocol_settings: Arc<ProtocolSettings>,
    version: Version,
    accounts: Arc<RwLock<HashMap<UInt160, Arc<HsmWalletAccount>>>>,
    default_account: RwLock<Option<UInt160>>,
}

impl HsmWallet {
    pub async fn from_runtime(
        runtime: HsmRuntime,
        settings: Arc<ProtocolSettings>,
    ) -> Result<Self> {
        let signer = Arc::clone(&runtime.signer);
        let keys = if let Some(key) = runtime.active_key.clone() {
            vec![key]
        } else {
            signer
                .list_keys()
                .await
                .map_err(|err| anyhow!(err.to_string()))?
        };

        if keys.is_empty() {
            return Err(anyhow!("HSM has no keys available"));
        }

        let mut accounts = HashMap::new();
        let mut default_account = None;
        for key in keys {
            let mut account = HsmWalletAccount::from_key_info(&signer, &settings, &key)?;
            let hash = account.script_hash();
            if default_account.is_none() {
                account.is_default = true;
                default_account = Some(hash);
            }
            accounts.insert(hash, Arc::new(account));
        }

        Ok(Self {
            name: "hsm-wallet".to_string(),
            signer,
            protocol_settings: settings,
            version: Version::new(1, 0, 0),
            accounts: Arc::new(RwLock::new(accounts)),
            default_account: RwLock::new(default_account),
        })
    }

    fn account_for_hash(&self, script_hash: &UInt160) -> WalletResult<Arc<HsmWalletAccount>> {
        self.accounts
            .read()
            .get(script_hash)
            .cloned()
            .ok_or(WalletError::AccountNotFound(*script_hash))
    }
}

#[async_trait::async_trait]
impl Wallet for HsmWallet {
    fn name(&self) -> &str {
        &self.name
    }

    fn path(&self) -> Option<&str> {
        None
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
            "HSM wallets do not support password changes".to_string(),
        ))
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        self.accounts.read().contains_key(script_hash)
    }

    async fn create_account(&self, _private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "HSM wallets do not support creating accounts".to_string(),
        ))
    }

    async fn create_account_with_contract(
        &self,
        _contract: Contract,
        _key_pair: Option<neo_core::wallets::KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "HSM wallets do not support creating accounts".to_string(),
        ))
    }

    async fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "HSM wallets do not support creating watch-only accounts".to_string(),
        ))
    }

    async fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
        Err(WalletError::Other(
            "HSM wallets do not support deleting accounts".to_string(),
        ))
    }

    async fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(WalletError::Other(
            "HSM wallets cannot be exported".to_string(),
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
            "HSM wallets do not track balances".to_string(),
        ))
    }

    async fn get_unclaimed_gas(&self) -> WalletResult<i64> {
        Err(WalletError::Other(
            "HSM wallets do not track unclaimed GAS".to_string(),
        ))
    }

    async fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "HSM wallets cannot import WIF keys".to_string(),
        ))
    }

    async fn import_nep2(
        &self,
        _nep2_key: &str,
        _password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(WalletError::Other(
            "HSM wallets cannot import NEP-2 keys".to_string(),
        ))
    }

    async fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        let account = self.account_for_hash(script_hash)?;
        if account.is_locked() {
            return Err(WalletError::AccountLocked);
        }
        account.sign_data(data).await
    }

    async fn sign_transaction(&self, transaction: &mut Transaction) -> WalletResult<()> {
        let signer_hashes: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();

        for hash in signer_hashes {
            if let Ok(account) = self.account_for_hash(&hash) {
                if account.is_locked() {
                    return Err(WalletError::AccountLocked);
                }
                let witness = account.sign_transaction(transaction).await?;
                transaction.add_witness(witness);
            }
        }

        Ok(())
    }

    async fn unlock(&self, password: &str) -> WalletResult<bool> {
        if !self.signer.is_locked() {
            return Ok(true);
        }

        self.signer
            .unlock(password)
            .await
            .map_err(|err| WalletError::Other(err.to_string()))?;
        Ok(true)
    }

    fn lock(&self) {
        self.signer.lock();
    }

    async fn verify_password(&self, password: &str) -> WalletResult<bool> {
        if !self.signer.is_locked() {
            return Ok(true);
        }

        match self.signer.unlock(password).await {
            Ok(_) => {
                self.signer.lock();
                Ok(true)
            }
            Err(neo_hsm::HsmError::InvalidPin) => Ok(false),
            Err(err) => Err(WalletError::Other(err.to_string())),
        }
    }

    async fn save(&self) -> WalletResult<()> {
        Ok(())
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        let hash = *self.default_account.read()?;
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
        *self.default_account.write() = Some(*script_hash);
        Ok(())
    }
}

pub struct HsmWalletAccount {
    script_hash: UInt160,
    label: Option<String>,
    is_default: bool,
    contract: Contract,
    key_id: String,
    public_key: Vec<u8>,
    signer: Arc<dyn HsmSigner>,
    protocol_settings: Arc<ProtocolSettings>,
}

impl HsmWalletAccount {
    fn from_key_info(
        signer: &Arc<dyn HsmSigner>,
        settings: &Arc<ProtocolSettings>,
        key: &HsmKeyInfo,
    ) -> WalletResult<Self> {
        let script_hash = UInt160::from_bytes(&key.script_hash)
            .map_err(|err| WalletError::Other(err.to_string()))?;
        let public_key = key.public_key.clone();
        let contract = signature_contract_from_public_key(&public_key)?;

        if contract.script_hash() != script_hash {
            return Err(WalletError::Other(format!(
                "Script hash mismatch for HSM key {}",
                key.key_id
            )));
        }

        Ok(Self {
            script_hash,
            label: key.label.clone(),
            is_default: false,
            contract,
            key_id: key.key_id.clone(),
            public_key,
            signer: Arc::clone(signer),
            protocol_settings: Arc::clone(settings),
        })
    }

    async fn sign_data(&self, data: &[u8]) -> WalletResult<Vec<u8>> {
        let signature = self
            .signer
            .sign(&self.key_id, data)
            .await
            .map_err(|err| WalletError::SigningFailed(err.to_string()))?;

        if signature.len() != 64 {
            return Err(WalletError::SigningFailed(
                "Invalid HSM signature length".to_string(),
            ));
        }
        Ok(signature)
    }

    async fn sign_transaction(&self, transaction: &Transaction) -> WalletResult<Witness> {
        let sign_data = get_sign_data_vec(transaction, self.protocol_settings.network)
            .map_err(|err| WalletError::SigningFailed(err.to_string()))?;
        let signature = self.sign_data(&sign_data).await?;
        let invocation = signature_invocation(&signature)?;
        Ok(Witness::new_with_scripts(
            invocation,
            self.contract.script.clone(),
        ))
    }
}

impl WalletAccount for HsmWalletAccount {
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
        self.is_default
    }

    fn set_is_default(&mut self, is_default: bool) {
        self.is_default = is_default;
    }

    fn is_locked(&self) -> bool {
        self.signer.is_locked()
    }

    fn has_key(&self) -> bool {
        true
    }

    fn get_key(&self) -> Option<neo_core::wallets::KeyPair> {
        None
    }

    fn contract(&self) -> Option<&Contract> {
        Some(&self.contract)
    }

    fn set_contract(&mut self, contract: Option<Contract>) {
        if let Some(contract) = contract {
            self.script_hash = contract.script_hash();
            self.contract = contract;
        }
    }

    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        &self.protocol_settings
    }

    fn unlock(&mut self, _password: &str) -> WalletResult<bool> {
        Ok(!self.signer.is_locked())
    }

    fn lock(&mut self) {
        self.signer.lock();
    }

    fn verify_password(&self, _password: &str) -> WalletResult<bool> {
        Ok(!self.signer.is_locked())
    }

    fn export_wif(&self) -> WalletResult<String> {
        Err(WalletError::Other(
            "HSM accounts cannot export WIF".to_string(),
        ))
    }

    fn export_nep2(&self, _password: &str) -> WalletResult<String> {
        Err(WalletError::Other(
            "HSM accounts cannot export NEP-2".to_string(),
        ))
    }

    fn create_witness(&self, transaction: &Transaction) -> WalletResult<Witness> {
        let sign_data = get_sign_data_vec(transaction, self.protocol_settings.network)
            .map_err(|err| WalletError::SigningFailed(err.to_string()))?;
        // Use block_in_place to avoid blocking the async runtime.
        // SAFETY: This is safe because block_in_place moves the task to a blocking thread pool.
        let signature = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.sign_data(&sign_data))
        })?;
        let invocation = signature_invocation(&signature)?;
        Ok(Witness::new_with_scripts(
            invocation,
            self.contract.script.clone(),
        ))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsm_integration::HsmRuntime;
    use neo_core::network::p2p::payloads::signer::Signer;
    use neo_core::smart_contract::helper::Helper as ContractHelper;
    use neo_core::WitnessScope;
    use neo_crypto::Secp256r1Crypto;
    use neo_hsm::{HsmConfig, SimulationSigner};
    use neo_vm::op_code::OpCode;

    #[tokio::test]
    async fn hsm_wallet_signs_and_builds_witness() {
        let signer = SimulationSigner::with_test_key().expect("simulation signer");
        let keys = signer.list_keys().await.expect("list keys");
        let key = keys.first().expect("key").clone();
        let settings = Arc::new(ProtocolSettings::default());
        let runtime = HsmRuntime {
            signer: Arc::new(signer),
            config: HsmConfig::default(),
            active_key: Some(key.clone()),
            address_version: settings.address_version,
        };

        let wallet = HsmWallet::from_runtime(runtime, Arc::clone(&settings))
            .await
            .expect("wallet");
        let script_hash = UInt160::from_bytes(&key.script_hash).expect("script hash");

        let payload = b"neo-hsm-wallet";
        let signature = wallet.sign(payload, &script_hash).await.expect("sign data");
        let signature_bytes: [u8; 64] = signature.as_slice().try_into().expect("sig");
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
