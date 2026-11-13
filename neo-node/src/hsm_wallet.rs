//! HSM-backed wallet implementation for RPC and local signing.
//!
//! Private keys never enter this process. Unsupported key import/export and
//! software-account creation operations fail explicitly.

use crate::hsm::{HsmError, HsmKeyInfo, HsmSigner};
use crate::hsm_integration::{HsmRuntime, run_hardware_operation};
use crate::wallet_scripts::signature_invocation;
use neo_crypto::{ECCurve, ECPoint};
use neo_primitives::{ContractParameterType, UInt160};
use neo_protocol::payloads::transaction::Transaction;
use neo_protocol::witness::Witness;
use neo_protocol::{ProtocolSettings, get_sign_data_vec};
use neo_smart_contract::contract::Contract;
use neo_wallets::{Helper, KeyPair, Version, Wallet, WalletAccount, WalletError, WalletResult};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Wallet view over an authenticated hardware signer.
pub struct HsmWallet {
    name: String,
    signer: Arc<dyn HsmSigner>,
    version: Version,
    accounts: Arc<RwLock<HashMap<UInt160, Arc<HsmWalletAccount>>>>,
    default_account: RwLock<Option<UInt160>>,
}

impl HsmWallet {
    /// Builds a wallet from an initialized runtime and its validated active key.
    pub async fn from_runtime(
        runtime: HsmRuntime,
        settings: Arc<ProtocolSettings>,
    ) -> WalletResult<Self> {
        let signer = Arc::clone(&runtime.signer);
        let account = Arc::new(HsmWalletAccount::from_key_info(
            &signer,
            &settings,
            &runtime.active_key,
            true,
        )?);
        let script_hash = account.script_hash();
        let accounts = HashMap::from([(script_hash, account)]);

        Ok(Self {
            name: "hsm-wallet".to_string(),
            signer,
            version: Version::new(1, 0, 0),
            accounts: Arc::new(RwLock::new(accounts)),
            default_account: RwLock::new(Some(script_hash)),
        })
    }

    /// Signs every transaction signer owned by this hardware wallet.
    pub fn sign_transaction(&self, transaction: &mut Transaction) -> WalletResult<()> {
        let signer_hashes: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        for hash in signer_hashes {
            if let Some(account) = self.accounts.read().get(&hash).cloned() {
                let witness = account.sign_transaction(transaction)?;
                transaction.add_witness(witness);
            }
        }
        Ok(())
    }

    fn account_for_hash(&self, script_hash: &UInt160) -> WalletResult<Arc<HsmWalletAccount>> {
        self.accounts
            .read()
            .get(script_hash)
            .cloned()
            .ok_or(WalletError::AccountNotFound(*script_hash))
    }

    fn update_default_account(&self, new_default: UInt160) {
        *self.default_account.write() = Some(new_default);
        for (hash, account) in self.accounts.read().iter() {
            account
                .is_default
                .store(*hash == new_default, Ordering::SeqCst);
        }
    }
}

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

    fn change_password(&self, _old_password: &str, _new_password: &str) -> WalletResult<bool> {
        Err(unsupported(
            "HSM wallets do not use password-encrypted host keys",
        ))
    }

    fn contains(&self, script_hash: &UInt160) -> bool {
        self.accounts.read().contains_key(script_hash)
    }

    fn create_account(&self, _private_key: &[u8]) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(unsupported(
            "HSM wallets cannot import private keys into hardware",
        ))
    }

    fn create_account_with_contract(
        &self,
        _contract: Contract,
        _key_pair: Option<KeyPair>,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(unsupported(
            "HSM accounts must be created and selected on the device",
        ))
    }

    fn create_account_watch_only(
        &self,
        _script_hash: UInt160,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(unsupported("HSM wallets do not create watch-only accounts"))
    }

    fn delete_account(&self, _script_hash: &UInt160) -> WalletResult<bool> {
        Err(unsupported("HSM wallets cannot delete on-device keys"))
    }

    fn export(&self, _path: &str, _password: &str) -> WalletResult<()> {
        Err(unsupported("HSM private keys cannot be exported"))
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

    fn import_wif(&self, _wif: &str) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(unsupported("HSM wallets cannot import WIF keys"))
    }

    fn import_nep2(
        &self,
        _nep2_key: &str,
        _password: &str,
    ) -> WalletResult<Arc<dyn WalletAccount>> {
        Err(unsupported("HSM wallets cannot import NEP-2 keys"))
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> WalletResult<Vec<u8>> {
        self.account_for_hash(script_hash)?.sign_data(data)
    }

    fn unlock(&self, password: &str) -> WalletResult<bool> {
        if !self.signer.is_locked() {
            return Ok(true);
        }
        let signer = Arc::clone(&self.signer);
        let password = password.to_string();
        run_hardware_operation(async move { signer.unlock(&password).await })
            .map_err(runtime_error)?
            .map_err(map_hsm_error)?;
        Ok(true)
    }

    fn lock(&self) {
        self.signer.lock();
    }

    fn verify_password(&self, password: &str) -> WalletResult<bool> {
        if !self.signer.is_locked() {
            return Ok(true);
        }
        let signer = Arc::clone(&self.signer);
        let password = password.to_string();
        let result = run_hardware_operation(async move { signer.unlock(&password).await })
            .map_err(runtime_error)?;
        self.signer.lock();
        match result {
            Ok(()) => Ok(true),
            Err(HsmError::InvalidPin) => Ok(false),
            Err(error) => Err(map_hsm_error(error)),
        }
    }

    fn save(&self) -> WalletResult<()> {
        Ok(())
    }

    fn get_default_account(&self) -> Option<Arc<dyn WalletAccount>> {
        let hash = (*self.default_account.read())?;
        self.get_account(&hash)
    }

    fn set_default_account(&self, script_hash: &UInt160) -> WalletResult<()> {
        if !self.contains(script_hash) {
            return Err(WalletError::AccountNotFound(*script_hash));
        }
        self.update_default_account(*script_hash);
        Ok(())
    }
}

/// A single-signature account whose private key remains inside an HSM.
pub struct HsmWalletAccount {
    script_hash: UInt160,
    label: Option<String>,
    is_default: AtomicBool,
    contract: Contract,
    key_id: String,
    signer: Arc<dyn HsmSigner>,
    protocol_settings: Arc<ProtocolSettings>,
}

impl HsmWalletAccount {
    fn from_key_info(
        signer: &Arc<dyn HsmSigner>,
        settings: &Arc<ProtocolSettings>,
        key: &HsmKeyInfo,
        is_default: bool,
    ) -> WalletResult<Self> {
        if key.algorithm != "secp256r1" {
            return Err(WalletError::Other(format!(
                "Unsupported HSM key algorithm: {}",
                key.algorithm
            )));
        }
        let script_hash = UInt160::from_bytes(&key.script_hash)
            .map_err(|error| WalletError::Other(error.to_string()))?;
        let contract = signature_contract_from_public_key(&key.public_key)?;
        if contract.script_hash() != script_hash {
            return Err(WalletError::Other(format!(
                "Script hash mismatch for HSM key {}",
                key.key_id
            )));
        }

        Ok(Self {
            script_hash,
            label: key.label.clone(),
            is_default: AtomicBool::new(is_default),
            contract,
            key_id: key.key_id.clone(),
            signer: Arc::clone(signer),
            protocol_settings: Arc::clone(settings),
        })
    }

    fn sign_data(&self, data: &[u8]) -> WalletResult<Vec<u8>> {
        if self.signer.is_locked() {
            return Err(WalletError::AccountLocked);
        }
        let signer = Arc::clone(&self.signer);
        let key_id = self.key_id.clone();
        let data = data.to_vec();
        let signature = run_hardware_operation(async move { signer.sign(&key_id, &data).await })
            .map_err(runtime_error)?
            .map_err(map_hsm_error)?;
        if signature.len() != 64 {
            return Err(WalletError::SigningFailed(format!(
                "HSM returned {} signature bytes; expected 64",
                signature.len()
            )));
        }
        Ok(signature)
    }

    fn sign_transaction(&self, transaction: &Transaction) -> WalletResult<Witness> {
        let sign_data = get_sign_data_vec(transaction, self.protocol_settings.network)
            .map_err(|error| WalletError::SigningFailed(error.to_string()))?;
        self.create_witness(&sign_data)
    }
}

impl WalletAccount for HsmWalletAccount {
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
        self.is_default.load(Ordering::SeqCst)
    }

    fn set_is_default(&mut self, is_default: bool) {
        self.is_default.store(is_default, Ordering::SeqCst);
    }

    fn is_locked(&self) -> bool {
        self.signer.is_locked()
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
        if let Some(contract) = contract
            && contract.script_hash() == self.script_hash
        {
            self.contract = contract;
        }
    }

    fn protocol_settings(&self) -> &Arc<ProtocolSettings> {
        &self.protocol_settings
    }

    fn unlock(&mut self, password: &str) -> WalletResult<bool> {
        if !self.signer.is_locked() {
            return Ok(true);
        }
        let signer = Arc::clone(&self.signer);
        let password = password.to_string();
        run_hardware_operation(async move { signer.unlock(&password).await })
            .map_err(runtime_error)?
            .map_err(map_hsm_error)?;
        Ok(true)
    }

    fn lock(&mut self) {
        self.signer.lock();
    }

    fn verify_password(&self, password: &str) -> WalletResult<bool> {
        if !self.signer.is_locked() {
            return Ok(true);
        }
        let signer = Arc::clone(&self.signer);
        let password = password.to_string();
        let result = run_hardware_operation(async move { signer.unlock(&password).await })
            .map_err(runtime_error)?;
        self.signer.lock();
        match result {
            Ok(()) => Ok(true),
            Err(HsmError::InvalidPin) => Ok(false),
            Err(error) => Err(map_hsm_error(error)),
        }
    }

    fn export_wif(&self) -> WalletResult<String> {
        Err(unsupported("HSM accounts cannot export WIF"))
    }

    fn export_nep2(&self, _password: &str) -> WalletResult<String> {
        Err(unsupported("HSM accounts cannot export NEP-2"))
    }

    fn create_witness(&self, sign_data: &[u8]) -> WalletResult<Witness> {
        let signature = self.sign_data(sign_data)?;
        let invocation = signature_invocation(&signature)?;
        Ok(Witness::new_with_scripts(
            invocation,
            self.contract.script.clone(),
        ))
    }
}

fn signature_contract_from_public_key(public_key: &[u8]) -> WalletResult<Contract> {
    let point = ECPoint::new(ECCurve::Secp256r1, public_key.to_vec())
        .map_err(|error| WalletError::SigningFailed(error.to_string()))?;
    Ok(Contract::create(
        vec![ContractParameterType::Signature],
        Contract::create_signature_redeem_script(point),
    ))
}

fn unsupported(message: &str) -> WalletError {
    WalletError::Other(message.to_string())
}

fn runtime_error(error: anyhow::Error) -> WalletError {
    WalletError::SigningFailed(error.to_string())
}

fn map_hsm_error(error: HsmError) -> WalletError {
    match error {
        HsmError::PinRequired | HsmError::NotInitialized => WalletError::AccountLocked,
        HsmError::InvalidPin => WalletError::InvalidPassword,
        other => WalletError::SigningFailed(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hsm::{HsmDeviceInfo, HsmDeviceType, HsmResult};
    use async_trait::async_trait;
    use neo_crypto::{Crypto, Secp256r1Crypto};
    use neo_primitives::WitnessScope;
    use neo_protocol::payloads::Signer;
    use neo_vm::OpCode;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PRIVATE_KEY: [u8; 32] = [7u8; 32];
    const PIN: &str = "123456";

    struct MockHardwareSigner {
        info: HsmDeviceInfo,
        key: HsmKeyInfo,
        locked: AtomicBool,
        ready: AtomicBool,
        fail_next_sign: AtomicBool,
        sign_calls: AtomicUsize,
    }

    impl MockHardwareSigner {
        fn new() -> Self {
            let public_key = Secp256r1Crypto::derive_public_key(&PRIVATE_KEY).expect("public key");
            let script_hash =
                crate::hsm::signer::script_hash_from_public_key(&public_key).expect("script hash");
            Self {
                info: HsmDeviceInfo {
                    device_type: HsmDeviceType::Pkcs11,
                    manufacturer: "Test".to_string(),
                    model: "Counted hardware boundary".to_string(),
                    serial_number: None,
                    firmware_version: None,
                    is_connected: true,
                    requires_pin: true,
                },
                key: HsmKeyInfo::new("neo-test-key", public_key, script_hash),
                locked: AtomicBool::new(true),
                ready: AtomicBool::new(false),
                fail_next_sign: AtomicBool::new(false),
                sign_calls: AtomicUsize::new(0),
            }
        }

        fn runtime(self: &Arc<Self>) -> HsmRuntime {
            HsmRuntime {
                signer: Arc::clone(self) as Arc<dyn HsmSigner>,
                active_key: self.key.clone(),
                address_version: ProtocolSettings::default().address_version,
            }
        }
    }

    #[async_trait]
    impl HsmSigner for MockHardwareSigner {
        fn device_info(&self) -> &HsmDeviceInfo {
            &self.info
        }

        fn is_ready(&self) -> bool {
            self.ready.load(Ordering::SeqCst)
        }

        async fn unlock(&self, pin: &str) -> HsmResult<()> {
            if pin != PIN {
                return Err(HsmError::InvalidPin);
            }
            self.locked.store(false, Ordering::SeqCst);
            self.ready.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn lock(&self) {
            self.locked.store(true, Ordering::SeqCst);
            self.ready.store(false, Ordering::SeqCst);
        }

        fn is_locked(&self) -> bool {
            self.locked.load(Ordering::SeqCst)
        }

        async fn list_keys(&self) -> HsmResult<Vec<HsmKeyInfo>> {
            Ok(vec![self.key.clone()])
        }

        async fn get_key(&self, key_id: &str) -> HsmResult<HsmKeyInfo> {
            if key_id == self.key.key_id {
                Ok(self.key.clone())
            } else {
                Err(HsmError::KeyNotFound(key_id.to_string()))
            }
        }

        async fn sign(&self, key_id: &str, data: &[u8]) -> HsmResult<Vec<u8>> {
            self.sign_calls.fetch_add(1, Ordering::SeqCst);
            if self.is_locked() {
                return Err(HsmError::PinRequired);
            }
            if key_id != self.key.key_id {
                return Err(HsmError::KeyNotFound(key_id.to_string()));
            }
            if self.fail_next_sign.swap(false, Ordering::SeqCst) {
                return Err(HsmError::SigningFailed("device rejected".to_string()));
            }
            Secp256r1Crypto::sign(data, &PRIVATE_KEY)
                .map(|signature| signature.to_vec())
                .map_err(|error| HsmError::SigningFailed(error.to_string()))
        }

        async fn get_public_key(&self, _key_id: &str) -> HsmResult<Vec<u8>> {
            Ok(self.key.public_key.clone())
        }

        async fn verify_device(&self) -> HsmResult<bool> {
            Ok(self.is_ready())
        }
    }

    fn unlocked_wallet(
        signer: &Arc<MockHardwareSigner>,
        settings: &Arc<ProtocolSettings>,
    ) -> HsmWallet {
        signer.lock();
        run_hardware_operation({
            let signer = Arc::clone(signer) as Arc<dyn HsmSigner>;
            async move { signer.unlock(PIN).await }
        })
        .expect("unlock future")
        .expect("unlock hardware");
        futures::executor::block_on(HsmWallet::from_runtime(
            signer.runtime(),
            Arc::clone(settings),
        ))
        .expect("HSM wallet")
    }

    #[test]
    fn wallet_signs_preimages_and_builds_witnesses_inside_current_thread_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("Tokio runtime");
        runtime.block_on(async {
            let signer = Arc::new(MockHardwareSigner::new());
            let settings = Arc::new(ProtocolSettings::default());
            let wallet = unlocked_wallet(&signer, &settings);
            let script_hash = UInt160::from_bytes(&signer.key.script_hash).expect("script hash");

            let payload = b"neo-hsm-current-thread-runtime";
            let signature = wallet.sign(payload, &script_hash).expect("sign preimage");
            let signature_bytes: [u8; 64] = signature.as_slice().try_into().expect("signature");
            assert!(
                Secp256r1Crypto::verify_prehash(
                    &Crypto::sha256(payload),
                    &signature_bytes,
                    &signer.key.public_key,
                )
                .expect("verify preimage")
            );

            let account = wallet.get_default_account().expect("default account");
            let witness = account.create_witness(payload).expect("trait witness");
            assert_eq!(witness.invocation_script.len(), 66);
            assert_eq!(&witness.invocation_script[2..], signature.as_slice());

            let mut transaction = Transaction::new();
            transaction.set_script(vec![OpCode::PUSH1.byte()]);
            transaction.set_valid_until_block(1);
            transaction.add_signer(Signer::new(script_hash, WitnessScope::CALLED_BY_ENTRY));
            wallet
                .sign_transaction(&mut transaction)
                .expect("sign transaction");
            assert_eq!(transaction.witnesses().len(), 1);

            let sign_data = get_sign_data_vec(&transaction, settings.network).expect("sign data");
            let invocation = &transaction.witnesses()[0].invocation_script;
            let transaction_signature: [u8; 64] =
                invocation[2..66].try_into().expect("transaction signature");
            assert!(
                Secp256r1Crypto::verify_prehash(
                    &Crypto::sha256(&sign_data),
                    &transaction_signature,
                    &signer.key.public_key,
                )
                .expect("verify transaction")
            );
            assert!(
                signer.sign_calls.load(Ordering::SeqCst) >= 3,
                "test must execute the hardware signing boundary"
            );
        });
    }

    #[test]
    fn lock_invalid_pin_and_device_failure_never_return_a_signature() {
        let signer = Arc::new(MockHardwareSigner::new());
        let settings = Arc::new(ProtocolSettings::default());
        let wallet = unlocked_wallet(&signer, &settings);
        let script_hash = UInt160::from_bytes(&signer.key.script_hash).expect("script hash");

        wallet.lock();
        let calls_before_locked_sign = signer.sign_calls.load(Ordering::SeqCst);
        assert!(matches!(
            wallet.sign(b"locked", &script_hash),
            Err(WalletError::AccountLocked)
        ));
        assert_eq!(
            signer.sign_calls.load(Ordering::SeqCst),
            calls_before_locked_sign
        );
        assert!(matches!(
            wallet.unlock("wrong"),
            Err(WalletError::InvalidPassword)
        ));
        assert!(signer.is_locked());

        assert!(wallet.unlock(PIN).expect("valid PIN"));
        signer.fail_next_sign.store(true, Ordering::SeqCst);
        let calls_before_failure = signer.sign_calls.load(Ordering::SeqCst);
        assert!(wallet.sign(b"device failure", &script_hash).is_err());
        assert_eq!(
            signer.sign_calls.load(Ordering::SeqCst),
            calls_before_failure + 1,
            "device failure path must reach the hardware boundary exactly once"
        );
    }
}
