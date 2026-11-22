//! Production adapters bridging the `neo-oracle` engine ports to neo-core.
//!
//! The generic Oracle service engine lives in the `neo-oracle` crate and is
//! generic over the seven port traits in `neo_oracle::ports`. The adapters in
//! this module are the concrete host-node implementations: they wrap
//! [`NeoSystem`], the native contracts, the `ApplicationEngine`, the trusted
//! signer client and the transaction router. They live in neo-core because
//! those facilities live here; the engine stays host-agnostic.
//!
//! - [`CoreSnapshot`]/[`CoreSnapshotFactory`]: wrap `PersistenceDataCache` snapshot
//!   creation and the native-contract state queries.
//! - [`CoreOracleWallet`]: replicates the reference `select_oracle_accounts`.
//! - [`CoreOracleSigner`]: replicates the reference frozen-sign-intent flow
//!   against the unified signing service.
//! - [`CoreResponseTxBuilder`]: host side of the reference `create_response_tx`;
//!   the quorum/witness/fee/verification domain rules live in
//!   `neo_oracle::create_response_tx` and this adapter only supplies the
//!   snapshot, native contract and engine ports. Verification runs on the
//!   canonical production native composition
//!   (`neo_native_contracts::native::composition::default_native_services`),
//!   so real state reads and witness hashes are available during the run.
//! - [`CoreTxRelay`]: relays through the transaction router.
//! - [`CoreSignatureVerifier`]: delegates to `ECPoint::verify_signature`.

use crate::neo_system::NeoSystem;
use async_trait::async_trait;
use neo_crypto::ECPoint;
use neo_native_contracts::native_contract::NativeContract;
use neo_native_contracts::{
    ContractManagement, LedgerContract, OracleContract, PolicyContract, Role, RoleManagement,
};
use neo_oracle::ports::{
    OracleSigner, OracleSnapshot, OracleWallet, ResponseTxBuilder, ResponseTxHost,
    SignatureVerifier, SnapshotFactory, TxRelay, VerificationOutcome,
};
use neo_oracle::{OracleRequest, OracleServiceError};
use neo_primitives::{CallFlags, OracleResponseCode, TriggerType};
use neo_primitives::{UInt160, UInt256};
use neo_protocol::payloads::OracleResponse;
use neo_protocol::payloads::Transaction;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::helper::Helper;
use neo_smart_contract::{Contract, ContractMethodDescriptor, ContractState};
use neo_storage::persistence::{PersistenceDataCache, StoreCache};
use neo_wallets::Wallet;
use neo_wallets::signing::{
    FrozenSignIntent, ProtocolContext, SIGNING_SCHEMA_VERSION, SignPayload, SignPurpose,
    TrustedSignerClient,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Ledger snapshot adapter wrapping a [`PersistenceDataCache`] plus protocol settings.
pub(crate) struct CoreSnapshot {
    data_cache: PersistenceDataCache,
    settings: ProtocolSettings,
}

impl CoreSnapshot {
    pub(crate) fn new(data_cache: PersistenceDataCache, settings: ProtocolSettings) -> Self {
        Self {
            data_cache,
            settings,
        }
    }

    /// Returns the underlying ledger snapshot. Used by
    /// [`CoreResponseTxBuilder`], which needs the raw `PersistenceDataCache` for the
    /// `ApplicationEngine` and native contracts.
    pub(crate) fn data_cache(&self) -> &PersistenceDataCache {
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
        OracleContract::new()
            .get_request(&self.data_cache, request_id)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))
    }

    fn get_requests(&self) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError> {
        OracleContract::new()
            .get_requests(&self.data_cache)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))
    }

    fn get_requests_by_url(
        &self,
        url: &str,
    ) -> Result<Vec<(u64, OracleRequest)>, OracleServiceError> {
        OracleContract::new()
            .get_requests_by_url(&self.data_cache, url)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))
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

    fn protocol_settings(&self) -> &ProtocolSettings {
        &self.settings
    }

    fn exec_fee_factor_with_decimals(&self) -> i64 {
        let height = LedgerContract::new()
            .current_index(&self.data_cache)
            .unwrap_or(0);
        PolicyContract::new()
            .get_exec_fee_factor_with_decimals_snapshot(&self.data_cache, &self.settings, height)
            .unwrap_or(
                (PolicyContract::DEFAULT_EXEC_FEE_FACTOR
                    * neo_smart_contract::application_engine::FEE_FACTOR as u32)
                    as i64,
            )
    }

    fn fee_per_byte(&self) -> i64 {
        PolicyContract::new()
            .get_fee_per_byte_snapshot(&self.data_cache)
            .unwrap_or(PolicyContract::DEFAULT_FEE_PER_BYTE as i64)
    }
}

/// Snapshot factory adapter: mirrors the reference `snapshot_cache` helper
/// (`system.store().snapshot()` → `StoreCache::new_from_snapshot` → `PersistenceDataCache`).
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

    fn oracle_key(&self, account: &UInt160) -> Option<neo_wallets::KeyPair> {
        self.wallet
            .get_account(account)
            .and_then(|account| (!account.is_locked()).then(|| account.get_key()).flatten())
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

/// Response transaction builder adapter: the host side of the reference
/// `create_response_tx`. Quorum, witness ordering, fee math and budget
/// downgrades are implemented once in `neo_oracle::create_response_tx`; this
/// adapter only supplies the snapshot, native-contract and engine ports.
pub(crate) struct CoreResponseTxBuilder {
    system: Arc<NeoSystem>,
}

impl CoreResponseTxBuilder {
    pub(crate) fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    /// Downcasts the opaque [`OracleSnapshot`] to this host's snapshot adapter.
    fn host_snapshot(snapshot: &dyn OracleSnapshot) -> Result<&CoreSnapshot, OracleServiceError> {
        core_snapshot(snapshot)
    }
}

impl ResponseTxHost for CoreResponseTxBuilder {
    fn oracle_contract_hash(&self) -> UInt160 {
        OracleContract::new().hash()
    }

    fn contract_state(
        &self,
        snapshot: &dyn OracleSnapshot,
        hash: &UInt160,
    ) -> Result<Option<ContractState>, OracleServiceError> {
        let snapshot = Self::host_snapshot(snapshot)?;
        ContractManagement::get_contract_from_snapshot(snapshot.data_cache(), hash)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))
    }

    fn execute_verification(
        &self,
        snapshot: &dyn OracleSnapshot,
        tx: &Transaction,
        contract: &ContractState,
        method: &ContractMethodDescriptor,
        call_flags: CallFlags,
    ) -> Result<VerificationOutcome, OracleServiceError> {
        let host_snapshot = Self::host_snapshot(snapshot)?;
        let mut engine = ApplicationEngine::new(
            TriggerType::Verification,
            Some(Arc::new(tx.clone())),
            Arc::new(host_snapshot.data_cache().clone()),
            None,
            self.system.settings().clone(),
            Helper::MAX_VERIFICATION_GAS,
            None,
            // Production oracle verification must observe real chain state and
            // resolve the transaction's verifying hashes, so it uses the
            // canonical composition (StandardNativeStateReader +
            // StandardWitnessHashResolver), never the no-op test builder.
            neo_native_contracts::native::composition::default_native_services(),
        )
        .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        engine
            .load_contract_method(contract.clone(), method.clone(), call_flags)
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;
        engine
            .execute()
            .map_err(|err| OracleServiceError::Processing(err.to_string()))?;

        Ok(VerificationOutcome {
            state: engine.state(),
            fee_consumed: engine.fee_consumed(),
        })
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
        let mut response = OracleResponse::new(response_id, response_code, response_result);
        let tx = neo_oracle::create_response_tx(
            self,
            snapshot,
            request,
            &mut response,
            oracle_nodes,
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
