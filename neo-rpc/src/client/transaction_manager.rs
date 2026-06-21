use crate::{Nep17Api, RpcClient, RpcError, TransactionManagerFactory};
use neo_crypto::ECPoint;
use neo_execution::{Contract, ContractParametersContext};
use neo_native_contracts::GasToken;
use neo_payloads::{
    Signer, Transaction, TransactionAttribute, VerifiableExt, Witness, get_sign_data_vec,
};
use neo_primitives::UInt160;
use neo_storage::persistence::DataCache;
use neo_wallets::KeyPair;
use neo_wallets::wallet_helper::WalletAddress as WalletHelper;
use num_bigint::BigInt;
use std::sync::Arc;

/// Sign item for transaction signing
#[derive(Clone)]
struct SignItem {
    /// The contract for signing
    contract: Contract,
    /// The key pairs for signing
    key_pairs: Vec<KeyPair>,
}

/// This class helps to create transaction with RPC API
/// Matches C# `TransactionManager`
pub struct TransactionManager {
    /// The RPC client instance
    _rpc_client: Arc<RpcClient>,

    /// The Transaction context to manage the witnesses
    context: ContractParametersContext,

    /// This container stores the keys for sign the transaction
    sign_store: Vec<SignItem>,

    /// The Transaction managed by this instance
    tx: Transaction,
}

impl TransactionManager {
    /// `TransactionManager` Constructor
    /// Matches C# constructor
    pub fn new(tx: Transaction, rpc_client: Arc<RpcClient>) -> Self {
        let snapshot = std::sync::Arc::new(neo_storage::persistence::DataCache::new(true));
        let context = ContractParametersContext::new(
            snapshot,
            tx.clone(),
            rpc_client.protocol_settings.network,
        );

        Self {
            _rpc_client: rpc_client,
            context,
            sign_store: Vec::new(),
            tx,
        }
    }

    /// Get the managed transaction
    pub const fn tx(&self) -> &Transaction {
        &self.tx
    }

    /// Helper function for one-off `TransactionManager` creation
    /// Matches C# `MakeTransactionAsync`
    pub async fn make_transaction(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        signers: Option<Vec<Signer>>,
        _attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<Self, RpcError> {
        let factory = TransactionManagerFactory::new(rpc_client);
        factory
            .make_transaction(script, &signers.unwrap_or_default())
            .await
    }

    /// Helper function for one-off `TransactionManager` creation with system fee
    /// Matches C# `MakeTransactionAsync` with systemFee parameter
    pub async fn make_transaction_with_fee(
        rpc_client: Arc<RpcClient>,
        script: &[u8],
        system_fee: i64,
        signers: Option<Vec<Signer>>,
        attributes: Option<Vec<TransactionAttribute>>,
    ) -> Result<Self, RpcError> {
        let factory = TransactionManagerFactory::new(rpc_client);
        let mut manager = factory
            .make_transaction(script, &signers.unwrap_or_default())
            .await?;
        manager.tx.set_system_fee(system_fee);

        if let Some(attrs) = attributes {
            manager.tx.set_attributes(attrs);
        }

        Ok(manager)
    }

    /// Add Signature
    /// Matches C# `AddSignature`
    pub fn add_signature(&mut self, key: &KeyPair) -> Result<&mut Self, RpcError> {
        let public_point = key.get_public_key_point()?;
        let contract = Contract::create_signature_contract(public_point);
        self.add_sign_item(contract, key.clone())?;
        Ok(self)
    }

    /// Add Multi-Signature
    /// Matches C# `AddMultiSig` with `KeyPair`
    pub fn add_multi_sig(
        &mut self,
        key: &KeyPair,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, RpcError> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);
        self.add_sign_item(contract, key.clone())?;
        Ok(self)
    }

    /// Add Multi-Signature with multiple keys
    /// Matches C# `AddMultiSig` with `KeyPair` array
    pub fn add_multi_sig_with_keys(
        &mut self,
        keys: Vec<KeyPair>,
        m: usize,
        public_keys: Vec<ECPoint>,
    ) -> Result<&mut Self, RpcError> {
        let contract = Contract::create_multi_sig_contract(m, &public_keys);

        for key in keys {
            self.add_sign_item(contract.clone(), key)?;
        }

        Ok(self)
    }

    /// Add witness with contract
    /// Matches C# `AddWitness`
    pub fn add_witness(&mut self, contract: Contract) -> Result<&mut Self, RpcError> {
        if !self.context.add_contract(contract) {
            return Err("AddWitness failed!".into());
        }
        Ok(self)
    }

    /// Add witness with script hash
    /// Matches C# `AddWitness` with `UInt160`.
    ///
    /// Note: Contract lookup requires an RPC call; use [`Self::add_witness_with_hash_async`].
    pub fn add_witness_with_hash(&mut self, script_hash: &UInt160) -> Result<&mut Self, RpcError> {
        let contract = Contract::create_with_hash(*script_hash, Vec::new());
        self.add_witness(contract)
    }

    /// Adds a witness by resolving the contract over RPC (required for contract accounts).
    pub async fn add_witness_with_hash_async(
        &mut self,
        script_hash: &UInt160,
    ) -> Result<&mut Self, RpcError> {
        let contract = self.get_contract_async(script_hash).await?;
        self.add_witness(contract)
    }

    /// Sign the transaction
    /// Matches C# `SignAsync`
    pub async fn sign(&mut self) -> Result<Transaction, RpcError> {
        let script_hashes = self.tx.script_hashes_for_verifying(&DataCache::new(true));
        let mut witnesses = Vec::with_capacity(script_hashes.len());
        for hash in &script_hashes {
            let verification_script = self.get_verification_script(hash);
            witnesses.push(Witness::new_with_scripts(Vec::new(), verification_script));
        }
        self.tx.set_witnesses(witnesses);

        let network_fee = self._rpc_client.calculate_network_fee(&self.tx).await?;
        self.tx.set_network_fee(network_fee);
        self.tx.set_witnesses(Vec::new());

        let sender = self
            .tx
            .sender()
            .ok_or_else(|| "Sender not specified in transaction".to_string())?;
        let gas_balance = Nep17Api::new(self._rpc_client.clone())
            .balance_of(&GasToken::new().hash(), &sender)
            .await?;
        let required_fee = BigInt::from(self.tx.system_fee() + self.tx.network_fee());
        if gas_balance < required_fee {
            let address = WalletHelper::to_address(
                &sender,
                self._rpc_client.protocol_settings.address_version,
            );
            return Err(format!("Insufficient GAS in address: {address}").into());
        }

        let sign_data = get_sign_data_vec(&self.tx, self._rpc_client.protocol_settings.network)?;
        for item in &self.sign_store {
            for key in &item.key_pairs {
                let signature = key.sign(&sign_data)?;
                let public_key = key.get_public_key_point()?;
                let added = self
                    .context
                    .add_signature(item.contract.clone(), public_key, signature)
                    .map_err(|err| format!("AddSignature failed: {err}"))?;
                if !added {
                    return Err("AddSignature failed!".into());
                }
            }
        }

        if !self.context.completed() {
            return Err("Please add signature or witness first!".into());
        }

        let final_witnesses = self
            .context
            .witnesses()
            .ok_or_else(|| "No witnesses available; context incomplete".to_string())?;
        self.tx.set_witnesses(final_witnesses);

        Ok(self.tx.clone())
    }

    // Helper methods

    fn add_sign_item(&mut self, contract: Contract, key: KeyPair) -> Result<(), RpcError> {
        let hash = contract.script_hash();
        let script_hashes = self.tx.script_hashes_for_verifying(&DataCache::new(true));
        if !script_hashes.contains(&hash) {
            return Err(format!("Add SignItem error: Mismatch ScriptHash ({hash})").into());
        }
        if let Some(item) = self
            .sign_store
            .iter_mut()
            .find(|i| i.contract.script_hash() == hash)
        {
            let exists = item
                .key_pairs
                .iter()
                .any(|candidate| candidate.private_key() == key.private_key());
            if !exists {
                item.key_pairs.push(key);
            }
        } else {
            let key_pairs = vec![key];
            self.sign_store.push(SignItem {
                contract: contract.clone(),
                key_pairs,
            });
        }

        self.context.add_contract(contract);
        Ok(())
    }

    fn get_verification_script(&self, hash: &UInt160) -> Vec<u8> {
        for item in &self.sign_store {
            if item.contract.script_hash() == *hash {
                return item.contract.script.clone();
            }
        }
        Vec::new()
    }

    async fn get_contract_async(&self, script_hash: &UInt160) -> Result<Contract, RpcError> {
        let state = self
            ._rpc_client
            .get_contract_state(&script_hash.to_string())
            .await?;

        let parameter_list = state
            .contract_state
            .manifest
            .abi
            .methods
            .iter()
            .find(|m| m.name.eq_ignore_ascii_case("verify"))
            .map(|method| method.parameters.iter().map(|p| p.param_type).collect())
            .unwrap_or_default();

        Ok(Contract::create_with_hash(*script_hash, parameter_list))
    }
}

#[cfg(test)]
#[path = "../tests/client/transaction_manager.rs"]
mod tests;
