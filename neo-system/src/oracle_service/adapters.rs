//! Production adapters bridging the `neo-oracle` engine ports to neo-core.
//!
//! The generic Oracle service engine lives in the `neo-oracle` crate and is
//! generic over the seven port traits in `neo_oracle::ports`. The adapters in
//! this module are the concrete host-node implementations: they wrap
//! [`NeoSystem`], the native contracts, the `ApplicationEngine`, the trusted
//! signer client and the transaction router. They live in neo-core because
//! those facilities live here; the engine stays host-agnostic.
//!
//! - [`CoreSnapshot`]/[`CoreSnapshotFactory`]: wrap `DataCache` snapshot
//!   creation and the native-contract state queries.
//! - [`CoreOracleWallet`]: replicates the reference `select_oracle_accounts`.
//! - [`CoreOracleSigner`]: replicates the reference frozen-sign-intent flow
//!   against the unified signing service.
//! - [`CoreResponseTxBuilder`]: ports the reference `create_response_tx` fee,
//!   witness and verification logic.
//! - [`CoreTxRelay`]: relays through the transaction router.
//! - [`CoreSignatureVerifier`]: delegates to `ECPoint::verify_signature`.

use crate::neo_system::NeoSystem;
use crate::network::p2p::payloads::{
    HEADER_SIZE, OracleResponse, OracleResponseCode, Signer, Transaction, TransactionAttribute,
    Witness,
    oracle_response::{MAX_RESULT_SIZE, OracleResponseExt},
};
use async_trait::async_trait;
use neo_core::VerifiableExt;
use neo_core::cryptography::ECPoint;
use neo_core::persistence::{DataCache, StoreCache};
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_basic_method::ContractBasicMethod;
use neo_core::smart_contract::native::native_contract::NativeContract;
use neo_core::smart_contract::native::{
    ContractManagement, LedgerContract, OracleContract, PolicyContract, Role, RoleManagement,
};
use neo_core::smart_contract::{ApplicationEngine, Contract, TriggerType};
use neo_core::wallets::Wallet;
use neo_io_crate::serializable::helper::{
    get_var_size, get_var_size_bytes, get_var_size_serializable_slice,
};
use neo_oracle::ports::{
    OracleSigner, OracleSnapshot, OracleWallet, ResponseTxBuilder, SignatureVerifier,
    SnapshotFactory, TxRelay,
};
use neo_oracle::{OracleRequest, OracleServiceError};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_signing::{
    FrozenSignIntent, ProtocolContext, SIGNING_SCHEMA_VERSION, SignPayload, SignPurpose,
    TrustedSignerClient,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

/// Converts an on-chain [`neo_core::smart_contract::native::OracleRequest`] into
/// the engine-side [`OracleRequest`] representation (structurally identical).
fn to_engine_request(request: &neo_core::smart_contract::native::OracleRequest) -> OracleRequest {
    OracleRequest {
        original_tx_id: request.original_tx_id,
        gas_for_response: request.gas_for_response,
        url: request.url.clone(),
        filter: request.filter.clone(),
        callback_contract: request.callback_contract,
        callback_method: request.callback_method.clone(),
        user_data: request.user_data.clone(),
    }
}

/// Ledger snapshot adapter wrapping a [`DataCache`] plus protocol settings.
pub(crate) struct CoreSnapshot {
    data_cache: DataCache,
    settings: ProtocolSettings,
}

impl CoreSnapshot {
    pub(crate) fn new(data_cache: DataCache, settings: ProtocolSettings) -> Self {
        Self {
            data_cache,
            settings,
        }
    }

    /// Returns the underlying ledger snapshot. Used by
    /// [`CoreResponseTxBuilder`], which needs the raw `DataCache` for the
    /// `ApplicationEngine` and native contracts.
    pub(crate) fn data_cache(&self) -> &DataCache {
        &self.data_cache
    }
}

impl OracleSnapshot for CoreSnapshot {
    fn ledger_height(&self) -> u32 {
        LedgerContract::new()
            .current_index(&self.data_cache)
            .unwrap_or(0)
            .saturating_add(1)
    }

    fn designated_oracles(&self, height: u32) -> Result<Vec<ECPoint>, OracleServiceError> {
        Ok(RoleManagement::new()
            .get_designated_by_role_at(&self.data_cache, Role::Oracle, height)
            .unwrap_or_default())
    }

    fn get_request(&self, request_id: u64) -> Result<Option<OracleRequest>, OracleServiceError> {
        Ok(OracleContract::new()
            .get_request(&self.data_cache, request_id)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?
            .as_ref()
            .map(to_engine_request))
    }

    fn get_requests(&self) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError> {
        Ok(OracleContract::new()
            .get_requests(&self.data_cache)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?
            .into_iter()
            .map(|(id, request)| (id, to_engine_request(&request)))
            .collect())
    }

    fn get_requests_by_url(
        &self,
        url: &str,
    ) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError> {
        Ok(OracleContract::new()
            .get_requests_by_url(&self.data_cache, url)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?
            .into_iter()
            .map(|(id, request)| (id, to_engine_request(&request)))
            .collect())
    }

    fn transaction_block_index(
        &self,
        tx_hash: &UInt256,
    ) -> Result<Option<u32>, OracleServiceError> {
        let state = LedgerContract::new()
            .get_transaction_state(&self.data_cache, tx_hash)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        Ok(state.map(|state| state.block_index()))
    }

    fn max_valid_until_block_increment(&self) -> u32 {
        PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(&self.data_cache, &self.settings)
            .unwrap_or(self.settings.max_valid_until_block_increment)
    }

    fn exec_fee_factor(&self) -> u32 {
        let height = LedgerContract::new()
            .current_index(&self.data_cache)
            .unwrap_or(0);
        PolicyContract::new()
            .get_exec_fee_factor_snapshot(&self.data_cache, &self.settings, height)
            .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR)
    }

    fn fee_per_byte(&self) -> i64 {
        PolicyContract::new()
            .get_fee_per_byte_snapshot(&self.data_cache)
            .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64)
    }
}

/// Snapshot factory adapter: mirrors the reference `snapshot_cache` helper
/// (`system.store().snapshot()` → `StoreCache::new_from_snapshot` → `DataCache`).
pub(crate) struct CoreSnapshotFactory {
    system: Arc<NeoSystem>,
}

impl CoreSnapshotFactory {
    pub(crate) fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }
}

impl SnapshotFactory for CoreSnapshotFactory {
    type Snapshot = CoreSnapshot;

    fn snapshot(&self) -> Self::Snapshot {
        let snapshot = self.system.store().snapshot();
        let store_cache = StoreCache::new_from_snapshot(snapshot);
        CoreSnapshot::new(
            store_cache.data_cache().clone(),
            self.system.settings().clone(),
        )
    }
}

/// Wallet adapter replicating the reference `select_oracle_accounts`: for
/// every designated oracle point the wallet is asked for the single-signature
/// account, and only unlocked accounts that hold a key are returned.
pub(crate) struct CoreOracleWallet {
    wallet: Arc<dyn Wallet>,
}

impl CoreOracleWallet {
    pub(crate) fn new(wallet: Arc<dyn Wallet>) -> Self {
        Self { wallet }
    }
}

impl OracleWallet for CoreOracleWallet {
    fn oracle_accounts(&self, designated_oracles: &[ECPoint]) -> Vec<(UInt160, ECPoint)> {
        designated_oracles
            .iter()
            .filter_map(|oracle| {
                let script_hash = Contract::create_signature_contract(oracle.clone()).script_hash();
                self.wallet.get_account(&script_hash).and_then(|account| {
                    (!account.is_locked() && account.has_key())
                        .then(|| (script_hash, oracle.clone()))
                })
            })
            .collect()
    }
}

/// Signer adapter replicating the reference `build_sign_intent` +
/// `resume_or_start` flow against the unified signing service. Fails closed
/// and never falls back to another key.
pub(crate) struct CoreOracleSigner {
    network: u32,
    client: TrustedSignerClient,
}

impl CoreOracleSigner {
    pub(crate) fn new(network: u32, client: TrustedSignerClient) -> Self {
        Self { network, client }
    }

    fn build_sign_intent(
        &self,
        account: &UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<FrozenSignIntent, OracleServiceError> {
        let account_selector = account.to_string();
        let key_ref = self
            .client
            .resolve_key(&account_selector, &SignPurpose::OracleResponse)
            .map_err(|error| OracleServiceError::Processing(error.to_string()))?;
        Ok(FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: self.network,
            purpose: SignPurpose::OracleResponse,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: "neo-n3-sign-data-v1".to_string(),
                bytes: payload,
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: protocol.to_string(),
                protocol_version: "1".to_string(),
                fields,
            },
            policy_version: self.client.policy_version().to_string(),
            fee_units: 0,
            quota_units: 1,
            account: Some(account_selector),
            targets: BTreeSet::new(),
        })
    }
}

#[async_trait]
impl OracleSigner for CoreOracleSigner {
    async fn sign_payload(
        &self,
        operation_id: String,
        account: UInt160,
        payload: Vec<u8>,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<Vec<u8>, OracleServiceError> {
        let intent = self.build_sign_intent(&account, payload, protocol, fields)?;
        self.client
            .resume_or_start(&operation_id, intent)
            .await
            .map(|response| response.signature)
            .map_err(|error| OracleServiceError::Processing(error.to_string()))
    }
}

/// Downcasts an opaque [`OracleSnapshot`] back to the neo-core snapshot
/// adapter (via the `Any` supertrait and trait upcasting).
fn core_snapshot(snapshot: &dyn OracleSnapshot) -> Result<&CoreSnapshot, OracleServiceError> {
    let any: &dyn std::any::Any = snapshot;
    any.downcast_ref::<CoreSnapshot>().ok_or_else(|| {
        OracleServiceError::Processing("oracle snapshot is not a neo-core snapshot".to_string())
    })
}

/// Response transaction builder adapter: full port of the reference
/// `create_response_tx` fee math, witness ordering and verification.
pub(crate) struct CoreResponseTxBuilder {
    system: Arc<NeoSystem>,
}

impl CoreResponseTxBuilder {
    pub(crate) fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    /// Builds the raw response transaction, downgrading `response` in place
    /// when the result is too large or the request budget is insufficient.
    fn create_response_tx(
        &self,
        snapshot: &DataCache,
        request: &OracleRequest,
        response: &mut OracleResponse,
        oracle_nodes: &[ECPoint],
        settings: &ProtocolSettings,
        use_current_height: bool,
    ) -> Result<Transaction, OracleServiceError> {
        let ledger = LedgerContract::new();
        let state = ledger
            .get_transaction_state(snapshot, &request.original_tx_id)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?
            .ok_or(OracleServiceError::RequestTransactionNotFound)?;

        let n = oracle_nodes.len();
        if n == 0 {
            return Err(OracleServiceError::Processing(
                "oracle nodes not designated".to_string(),
            ));
        }
        let m = n - (n - 1) / 3;
        let oracle_sign_contract = Contract::create_multi_sig_contract(m, oracle_nodes);

        let height = ledger.current_index(snapshot).unwrap_or(0);
        let max_vub = PolicyContract::new()
            .get_max_valid_until_block_increment_snapshot(snapshot, settings)
            .unwrap_or(settings.max_valid_until_block_increment);
        let mut valid_until_block = state.block_index().saturating_add(max_vub);
        while use_current_height && valid_until_block <= height {
            valid_until_block = valid_until_block.saturating_add(max_vub);
        }

        let mut tx = Transaction::new();
        tx.set_version(0);
        tx.set_nonce(response.id as u32);
        tx.set_valid_until_block(valid_until_block);
        tx.set_signers(vec![
            Signer::new(OracleContract::new().hash(), WitnessScope::NONE),
            Signer::new(oracle_sign_contract.script_hash(), WitnessScope::NONE),
        ]);
        tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
        tx.set_script(OracleResponse::get_fixed_script());
        tx.set_witnesses(vec![Witness::empty(), Witness::new()]);

        let witness_map: HashMap<UInt160, Witness> = vec![
            (
                oracle_sign_contract.script_hash(),
                Witness::new_with_scripts(Vec::new(), oracle_sign_contract.script.clone()),
            ),
            (OracleContract::new().hash(), Witness::empty()),
        ]
        .into_iter()
        .collect();

        let hashes = tx.script_hashes_for_verifying(snapshot);
        let mut witnesses = Vec::with_capacity(hashes.len());
        for hash in hashes.iter() {
            let witness = witness_map
                .get(hash)
                .cloned()
                .unwrap_or_else(Witness::empty);
            witnesses.push(witness);
        }
        tx.set_witnesses(witnesses);

        let oracle_contract =
            ContractManagement::get_contract_from_snapshot(snapshot, &OracleContract::new().hash())
                .map_err(|err| OracleServiceError::Processing(err.to_string()))?
                .ok_or_else(|| {
                    OracleServiceError::BuildFailed("oracle contract missing".to_string())
                })?;

        let mut engine = ApplicationEngine::new(
            TriggerType::Verification,
            Some(Arc::new(tx.clone())),
            Arc::new(snapshot.clone()),
            None,
            settings.clone(),
            neo_core::smart_contract::helper::Helper::MAX_VERIFICATION_GAS,
            None,
            neo_core::smart_contract::native::composition::default_native_services(),
        )
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        let mut abi = oracle_contract.manifest.abi.clone();
        let method = abi
            .get_method(
                ContractBasicMethod::VERIFY,
                ContractBasicMethod::VERIFY_P_COUNT,
            )
            .ok_or_else(|| {
                OracleServiceError::BuildFailed("oracle verify method missing".to_string())
            })?
            .clone();

        engine
            .load_contract_method(oracle_contract.clone(), method, CallFlags::NONE)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        if engine.state() != neo_vm::VmState::HALT {
            return Err(OracleServiceError::BuildFailed(
                "oracle verify failed".to_string(),
            ));
        }

        let comp1 = engine.fee_consumed();
        tx.set_network_fee(comp1);

        let exec_fee_factor = PolicyContract::new()
            .get_exec_fee_factor_snapshot(snapshot, settings, height)
            .unwrap_or(PolicyContract::DEFAULT_EXEC_FEE_FACTOR)
            as i64;
        let multi_sig_cost =
            neo_core::smart_contract::helper::Helper::multi_signature_contract_cost(
                m as i32, n as i32,
            );
        let comp2 = exec_fee_factor * multi_sig_cost;
        tx.set_network_fee(tx.network_fee().saturating_add(comp2));

        let size_inv = 66 * m;
        let oracle_witness_size = Witness::empty().size();
        let mut size = HEADER_SIZE
            + get_var_size_serializable_slice(tx.signers())
            + get_var_size_bytes(tx.script())
            + get_var_size(hashes.len() as u64)
            + oracle_witness_size
            + get_var_size(size_inv as u64)
            + size_inv
            + get_var_size(oracle_sign_contract.script.len() as u64)
            + oracle_sign_contract.script.len();

        let fee_per_byte = PolicyContract::new()
            .get_fee_per_byte_snapshot(snapshot)
            .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64);

        if response.result.len() > MAX_RESULT_SIZE {
            response.code = OracleResponseCode::ResponseTooLarge;
            response.result = Vec::new();
            tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
        } else if tx.network_fee()
            + ((size + get_var_size_serializable_slice(tx.attributes())) as i64 * fee_per_byte)
            > request.gas_for_response
        {
            response.code = OracleResponseCode::InsufficientFunds;
            response.result = Vec::new();
            tx.set_attributes(vec![TransactionAttribute::OracleResponse(response.clone())]);
        }

        size += get_var_size_serializable_slice(tx.attributes());
        let comp3 = size as i64 * fee_per_byte;
        let final_network_fee = tx.network_fee().saturating_add(comp3);
        tx.set_network_fee(final_network_fee);
        tx.set_system_fee(request.gas_for_response - final_network_fee);

        Ok(tx)
    }
}

impl ResponseTxBuilder for CoreResponseTxBuilder {
    fn build_response_tx(
        &self,
        snapshot: &dyn OracleSnapshot,
        request: &OracleRequest,
        response_id: u64,
        response_code: OracleResponseCode,
        response_result: Vec<u8>,
        oracle_nodes: &[ECPoint],
        _network: u32,
        use_current_height: bool,
    ) -> Result<(Transaction, OracleResponseCode), OracleServiceError> {
        let core_snapshot = core_snapshot(snapshot)?;
        let settings = self.system.settings();
        let mut response = OracleResponse::new(response_id, response_code, response_result);
        let tx = self.create_response_tx(
            core_snapshot.data_cache(),
            request,
            &mut response,
            oracle_nodes,
            settings,
            use_current_height,
        )?;
        Ok((tx, response.code))
    }
}

/// Relay adapter: submits through the transaction router's preverify queue.
pub(crate) struct CoreTxRelay {
    system: Arc<NeoSystem>,
}

impl CoreTxRelay {
    pub(crate) fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }
}

impl TxRelay for CoreTxRelay {
    fn relay_transaction(&self, tx: Transaction) -> Result<(), OracleServiceError> {
        self.system
            .tx_router_actor()
            .try_enqueue_preverify(tx, true)
            .map_err(|error| OracleServiceError::Processing(error.to_string()))
    }
}

/// Verifier adapter: delegates to `ECPoint::verify_signature`. The engine
/// performs the 64-byte length check before calling.
pub(crate) struct CoreSignatureVerifier;

impl SignatureVerifier for CoreSignatureVerifier {
    fn verify(&self, public_key: &ECPoint, message: &[u8], signature: &[u8]) -> bool {
        public_key
            .verify_signature(message, signature)
            .unwrap_or(false)
    }
}
