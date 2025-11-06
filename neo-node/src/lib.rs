use std::{collections::BTreeMap, fs, net::SocketAddr, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::{anyhow, Context, Result};
use axum::{
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use hex::decode;
use neo_base::{encoding::ToHex, hash::Hash160, AddressVersion};
use neo_consensus::{
    load_engine, persist_engine, ChangeViewReason, ConsensusState, DbftEngine, MessageKind,
    SnapshotKey, Validator, ValidatorId, ValidatorSet, ViewNumber,
};
use neo_crypto::{
    ecc256::{PrivateKey, PublicKey},
    scrypt::ScryptParams,
    Keypair,
};
use neo_runtime::{BlockSummary, PendingTransaction, Runtime, RuntimeStats};
use neo_store::{ColumnId, SledStore, Store};
use neo_wallet::{SignerScopes, WalletError, WalletStorage};

use axum::http::StatusCode;
#[cfg(test)]
use neo_store::{Column, MemoryStore};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::RwLock, task::JoinHandle, time::Duration};
use tracing::info;

type DynStore = dyn Store + Send + Sync;
type SharedStore = Arc<DynStore>;

pub const DEFAULT_STAGE_STALE_AFTER_MS: u128 = 5_000;
const DEFAULT_ADDRESS_VERSION: AddressVersion = AddressVersion::MAINNET;
const DEFAULT_SCRYPT_PARAMS: ScryptParams = ScryptParams {
    n: 16_384,
    r: 8,
    p: 8,
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StageState {
    Inactive,
    Pending,
    Complete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageStatus {
    pub state: StageState,
    pub expected: Option<usize>,
    pub responded: usize,
    pub missing: Vec<u16>,
    pub last_updated: DateTime<Utc>,
    pub age_ms: u128,
    pub stale: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorDescriptor {
    pub id: u16,
    pub public_key: String,
    pub script_hash: String,
    pub alias: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidatorConfig {
    pub id: u16,
    pub public_key: String,
    #[serde(default)]
    pub alias: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeStatus {
    pub network: String,
    pub height: u64,
    pub view: u32,
    pub connected_peers: usize,
    pub timestamp: DateTime<Utc>,
    pub base_fee: u64,
    pub byte_fee: u64,
    pub mempool_size: usize,
    pub total_transactions: u64,
    pub total_fees: u64,
    #[serde(default)]
    pub consensus_participation: BTreeMap<String, Vec<u16>>,
    #[serde(default)]
    pub consensus_tallies: BTreeMap<String, usize>,
    #[serde(default)]
    pub consensus_quorum: usize,
    #[serde(default)]
    pub consensus_primary: Option<u16>,
    #[serde(default)]
    pub consensus_validators: Vec<ValidatorDescriptor>,
    #[serde(default)]
    pub consensus_missing: BTreeMap<String, Vec<u16>>,
    #[serde(default)]
    pub consensus_expected: BTreeMap<String, usize>,
    #[serde(default)]
    pub consensus_change_view_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub consensus_change_view_total: u64,
    #[serde(default)]
    pub consensus_change_view_reasons: BTreeMap<u16, ChangeViewReason>,
    #[serde(default)]
    pub consensus_stages: BTreeMap<String, StageStatus>,
    #[serde(default)]
    pub consensus_stale_threshold_ms: Option<u128>,
    #[serde(default)]
    pub consensus_stage_timestamp: Option<DateTime<Utc>>,
}

impl NodeStatus {
    fn new(network: String) -> Self {
        Self {
            network,
            height: 0,
            view: 0,
            connected_peers: 0,
            timestamp: Utc::now(),
            base_fee: 0,
            byte_fee: 0,
            mempool_size: 0,
            total_transactions: 0,
            total_fees: 0,
            consensus_participation: BTreeMap::new(),
            consensus_tallies: BTreeMap::new(),
            consensus_quorum: 0,
            consensus_primary: None,
            consensus_validators: Vec::new(),
            consensus_missing: BTreeMap::new(),
            consensus_expected: BTreeMap::new(),
            consensus_change_view_reason_counts: BTreeMap::new(),
            consensus_change_view_total: 0,
            consensus_change_view_reasons: BTreeMap::new(),
            consensus_stages: BTreeMap::new(),
            consensus_stale_threshold_ms: None,
            consensus_stage_timestamp: None,
        }
    }

    fn from_state(
        network: String,
        engine: &DbftEngine,
        runtime: &Runtime,
        stale_after_ms: u128,
    ) -> Self {
        let mut status = Self::new(network);
        let participation = engine.participation();
        let tallies = engine.tallies();
        let (missing, expected, stages) = gather_consensus_metrics(engine, &participation);
        let validators = describe_validators(engine.state().validators());
        let change_view_reasons = engine.change_view_reasons();
        let change_view_reason_counts = engine.change_view_reason_counts();
        let change_view_total = engine.change_view_total();
        status.apply_consensus(
            engine.state().height(),
            engine.state().view(),
            participation,
            tallies,
            engine.quorum_threshold(),
            engine.primary(),
            validators,
            missing,
            expected,
            change_view_reason_counts,
            change_view_reasons,
            change_view_total,
            stages,
            stale_after_ms,
        );
        status.apply_runtime(&runtime.stats());
        status
    }

    fn apply_consensus(
        &mut self,
        height: u64,
        view: ViewNumber,
        participation: BTreeMap<MessageKind, Vec<ValidatorId>>,
        tallies: BTreeMap<MessageKind, usize>,
        quorum: usize,
        primary: Option<ValidatorId>,
        validators: Vec<ValidatorDescriptor>,
        missing: BTreeMap<MessageKind, Vec<u16>>,
        expected: BTreeMap<MessageKind, usize>,
        change_view_reason_counts: BTreeMap<ChangeViewReason, usize>,
        change_view_reasons: BTreeMap<ValidatorId, ChangeViewReason>,
        change_view_total: u64,
        stages: BTreeMap<MessageKind, StageStatus>,
        stale_after_ms: u128,
    ) {
        self.height = height;
        self.view = view.0;
        let mut participation_map = BTreeMap::new();
        for (kind, validators) in participation {
            participation_map.insert(
                format!("{kind:?}"),
                validators.into_iter().map(|id| id.0).collect(),
            );
        }
        self.consensus_participation = participation_map;

        let mut tallies_map = BTreeMap::new();
        for (kind, count) in tallies {
            tallies_map.insert(format!("{kind:?}"), count);
        }
        self.consensus_tallies = tallies_map;

        self.consensus_quorum = quorum;
        self.consensus_primary = primary.map(|id| id.0);
        self.consensus_validators = validators;
        let mut missing_map = BTreeMap::new();
        for (kind, validators) in missing {
            missing_map.insert(format!("{kind:?}"), validators);
        }
        self.consensus_missing = missing_map;

        let mut expected_map = BTreeMap::new();
        for (kind, count) in expected {
            expected_map.insert(format!("{kind:?}"), count);
        }
        self.consensus_expected = expected_map;

        let mut reason_counts_map = BTreeMap::new();
        for (reason, count) in change_view_reason_counts {
            reason_counts_map.insert(reason.to_string(), count);
        }
        self.consensus_change_view_reason_counts = reason_counts_map;
        self.consensus_change_view_total = change_view_total;

        let mut reasons_map = BTreeMap::new();
        for (validator, reason) in change_view_reasons {
            reasons_map.insert(validator.0, reason);
        }
        self.consensus_change_view_reasons = reasons_map;

        let now = Utc::now();
        let mut stage_map = BTreeMap::new();
        for (kind, mut status) in stages {
            let key = format!("{kind:?}");
            if let Some(prev) = self.consensus_stages.get(&key) {
                if prev.state == status.state
                    && prev.expected == status.expected
                    && prev.responded == status.responded
                    && prev.missing == status.missing
                {
                    status.last_updated = prev.last_updated;
                }
            }
            status.age_ms = now
                .signed_duration_since(status.last_updated)
                .num_milliseconds()
                .max(0) as u128;
            status.stale = if stale_after_ms == 0 {
                false
            } else {
                status.age_ms > stale_after_ms
            };
            stage_map.insert(key, status);
        }
        self.consensus_stages = stage_map;
        self.consensus_stale_threshold_ms = Some(stale_after_ms);
        self.consensus_stage_timestamp = Some(now);
        self.timestamp = Utc::now();
    }

    fn apply_runtime(&mut self, stats: &RuntimeStats) {
        self.height = stats.height;
        self.base_fee = stats.base_fee;
        self.byte_fee = stats.byte_fee;
        self.mempool_size = stats.mempool_size;
        self.total_transactions = stats.total_transactions;
        self.total_fees = stats.total_fees;
        self.timestamp = Utc::now();
    }
}

fn gather_consensus_metrics(
    engine: &DbftEngine,
    participation: &BTreeMap<MessageKind, Vec<ValidatorId>>,
) -> (
    BTreeMap<MessageKind, Vec<u16>>,
    BTreeMap<MessageKind, usize>,
    BTreeMap<MessageKind, StageStatus>,
) {
    let kinds = [
        MessageKind::PrepareRequest,
        MessageKind::PrepareResponse,
        MessageKind::Commit,
        MessageKind::ChangeView,
    ];
    let mut missing = BTreeMap::new();
    let mut expected_counts = BTreeMap::new();
    let mut stages = BTreeMap::new();
    let now = Utc::now();
    for kind in kinds {
        let responded = participation
            .get(&kind)
            .map(|validators| validators.len())
            .unwrap_or(0);
        let missing_ids = engine
            .missing_validators(kind)
            .into_iter()
            .map(|id| id.0)
            .collect::<Vec<_>>();
        let expected = engine.expected_participants(kind);
        let state = match expected {
            Some(ref participants) if !participants.is_empty() => {
                expected_counts.insert(kind, participants.len());
                if missing_ids.is_empty() && responded > 0 {
                    StageState::Complete
                } else {
                    StageState::Pending
                }
            }
            _ => {
                if responded > 0 {
                    StageState::Complete
                } else {
                    StageState::Inactive
                }
            }
        };
        missing.insert(kind, missing_ids.clone());
        stages.insert(
            kind,
            StageStatus {
                state,
                expected: expected.map(|v| v.len()),
                responded,
                missing: missing_ids,
                last_updated: now,
                age_ms: 0,
                stale: false,
            },
        );
    }
    (missing, expected_counts, stages)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeConfig {
    pub network: String,
    pub network_magic: u32,
    pub rpc_bind: SocketAddr,
    pub snapshot_path: PathBuf,
    pub stage_stale_after_ms: u128,
    #[serde(default)]
    pub validators: Option<Vec<ValidatorConfig>>,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            network: "mainnet".to_string(),
            network_magic: Self::magic_for_network("mainnet"),
            rpc_bind: ([127, 0, 0, 1], 20332).into(),
            snapshot_path: Self::default_snapshot_path("mainnet"),
            stage_stale_after_ms: DEFAULT_STAGE_STALE_AFTER_MS,
            validators: None,
        }
    }
}

impl NodeConfig {
    pub fn magic_for_network(name: &str) -> u32 {
        match name {
            "mainnet" => 860_833_102,
            "testnet" => 195_135_2142,
            "privatenet" => 767_083_703,
            _ => 0,
        }
    }

    pub fn snapshot_key(&self) -> SnapshotKey {
        SnapshotKey {
            network: self.network_magic,
        }
    }

    pub fn default_snapshot_path(network: &str) -> PathBuf {
        PathBuf::from(format!("data/{network}/consensus"))
    }
}

pub struct Node {
    config: NodeConfig,
    state: Arc<RwLock<NodeStatus>>,
    store: SharedStore,
    consensus: Arc<RwLock<DbftEngine>>,
    snapshot_key: SnapshotKey,
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    runtime: Arc<RwLock<Runtime>>,
}

impl Node {
    pub fn new(config: NodeConfig) -> Result<Self> {
        let path = config.snapshot_path.clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create snapshot directory {}", parent.display()))?;
        }
        let sled_store = Arc::new(SledStore::open(&path).map_err(anyhow::Error::from)?);
        let store: SharedStore = sled_store.clone();
        Self::with_store(config, store)
    }

    pub fn with_store(config: NodeConfig, store: SharedStore) -> Result<Self> {
        let snapshot_key = config.snapshot_key();
        let validators = match &config.validators {
            Some(entries) => {
                configured_validators(entries).with_context(|| "invalid validator configuration")?
            }
            None => default_validators(),
        };
        let maybe_engine = load_engine(store.as_ref(), validators.clone(), snapshot_key)
            .map_err(anyhow::Error::from)?;
        let (engine, has_snapshot) = match maybe_engine {
            Some(engine) => (engine, true),
            None => (
                DbftEngine::new(ConsensusState::new(0, ViewNumber::ZERO, validators.clone())),
                false,
            ),
        };

        if !has_snapshot {
            persist_engine(store.as_ref(), snapshot_key, &engine)
                .map_err(anyhow::Error::from)
                .context("persist initial consensus snapshot")?;
        }

        let wallet = WalletStorage::<DynStore>::open(
            store.clone(),
            ColumnId::new("wallet.keystore"),
            b"default".to_vec(),
        )?;
        let mut runtime = Runtime::new(100, 1);
        runtime.sync_height(engine.state().height());
        let status = NodeStatus::from_state(
            config.network.clone(),
            &engine,
            &runtime,
            config.stage_stale_after_ms,
        );

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(status)),
            store,
            consensus: Arc::new(RwLock::new(engine)),
            snapshot_key,
            wallet: Arc::new(RwLock::new(wallet)),
            runtime: Arc::new(RwLock::new(runtime)),
        })
    }

    pub async fn run(self) -> Result<()> {
        let Node {
            config,
            state,
            store,
            consensus,
            snapshot_key,
            wallet,
            runtime,
        } = self;

        let stage_stale_after_ms = config.stage_stale_after_ms;
        let rpc_state = state.clone();
        let router = build_router(
            rpc_state.clone(),
            wallet.clone(),
            runtime.clone(),
            stage_stale_after_ms,
        );
        let listener = TcpListener::bind(config.rpc_bind).await?;

        info!(target: "neo-node", "RPC listening on {}", config.rpc_bind);

        let rpc_handle: JoinHandle<Result<()>> = tokio::spawn(async move {
            axum::serve(listener, router.into_make_service())
                .await
                .map_err(anyhow::Error::from)
        });

        let services: JoinHandle<Result<()>> = tokio::spawn(async move {
            run_background_tasks(
                rpc_state,
                consensus,
                store,
                snapshot_key,
                runtime,
                stage_stale_after_ms,
            )
            .await
        });

        tokio::select! {
            res = rpc_handle => {
                res??;
                Ok(())
            },
            res = services => {
                res??;
                Ok(())
            },
        }
    }
}

pub async fn run(config: NodeConfig) -> Result<()> {
    Node::new(config)?.run().await
}

fn build_router(
    state: Arc<RwLock<NodeStatus>>,
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    runtime: Arc<RwLock<Runtime>>,
    default_stale_after_ms: u128,
) -> Router {
    let wallet_for_accounts = wallet.clone();
    let wallet_for_accounts_detail = wallet.clone();
    let wallet_for_import_wif = wallet.clone();
    let wallet_for_import_nep2 = wallet.clone();
    let wallet_for_export_wif = wallet.clone();
    let wallet_for_export_nep2 = wallet.clone();
    let wallet_for_update_signer = wallet.clone();
    Router::new()
        .route("/status", {
            let state = state.clone();
            get(move || status_handler(state.clone()))
        })
        .route("/consensus", {
            let state = state.clone();
            let default = default_stale_after_ms;
            get(move |params| consensus_handler(state.clone(), params, default))
        })
        .route(
            "/wallet/accounts",
            get(move || wallet_accounts_handler(wallet_for_accounts.clone())),
        )
        .route(
            "/wallet/import/wif",
            post(move |payload| wallet_import_wif_handler(wallet_for_import_wif.clone(), payload)),
        )
        .route(
            "/wallet/import/nep2",
            post(move |payload| {
                wallet_import_nep2_handler(wallet_for_import_nep2.clone(), payload)
            }),
        )
        .route(
            "/wallet/accounts/detail",
            post(move |payload| wallet_accounts_detail_handler(wallet_for_accounts_detail.clone(), payload)),
        )
        .route(
            "/wallet/export/wif",
            post(move |payload| wallet_export_wif_handler(wallet_for_export_wif.clone(), payload)),
        )
        .route(
            "/wallet/export/nep2",
            post(move |payload| {
                wallet_export_nep2_handler(wallet_for_export_nep2.clone(), payload)
            }),
        )
        .route(
            "/wallet/update/signer",
            post(move |payload| wallet_update_signer_handler(wallet_for_update_signer.clone(), payload)),
        )
        .route(
            "/wallet/pending",
            get(move || wallet_pending_handler(runtime.clone())),
        )
}

async fn status_handler(state: Arc<RwLock<NodeStatus>>) -> Json<NodeStatus> {
    let mut snapshot = state.read().await.clone();
    if snapshot.consensus_stale_threshold_ms.is_none() {
        snapshot.consensus_stale_threshold_ms = Some(DEFAULT_STAGE_STALE_AFTER_MS);
    }
    Json(snapshot)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConsensusStatus {
    pub height: u64,
    pub view: u32,
    pub quorum: usize,
    pub primary: Option<u16>,
    pub validators: Vec<ValidatorDescriptor>,
    pub tallies: BTreeMap<String, usize>,
    pub participation: BTreeMap<String, Vec<u16>>,
    pub missing: BTreeMap<String, Vec<u16>>,
    pub expected: BTreeMap<String, usize>,
    pub stages: BTreeMap<String, StageStatus>,
    pub stale_threshold_ms: Option<u128>,
    #[serde(default)]
    pub change_view_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    pub change_view_reasons: BTreeMap<u16, ChangeViewReason>,
    #[serde(default)]
    pub change_view_total: u64,
}

#[derive(Deserialize)]
struct ConsensusQuery {
    stale_threshold_ms: Option<u64>,
}

async fn consensus_handler(
    state: Arc<RwLock<NodeStatus>>,
    params: axum::extract::Query<ConsensusQuery>,
    default_threshold: u128,
) -> Json<ConsensusStatus> {
    let override_threshold = params.stale_threshold_ms.map(|ms| ms as u128);
    let mut snapshot = state.read().await.clone();
    let effective = override_threshold.unwrap_or_else(|| {
        snapshot
            .consensus_stale_threshold_ms
            .unwrap_or(default_threshold)
    });
    if override_threshold.is_some() {
        snapshot.consensus_stale_threshold_ms = override_threshold;
    }
    let mut stages = snapshot.consensus_stages.clone();
    for status in stages.values_mut() {
        status.stale = if effective == 0 {
            false
        } else {
            status.age_ms > effective
        };
    }
    Json(ConsensusStatus {
        height: snapshot.height,
        view: snapshot.view,
        quorum: snapshot.consensus_quorum,
        primary: snapshot.consensus_primary,
        validators: snapshot.consensus_validators.clone(),
        tallies: snapshot.consensus_tallies.clone(),
        participation: snapshot.consensus_participation.clone(),
        missing: snapshot.consensus_missing.clone(),
        expected: snapshot.consensus_expected.clone(),
        stages,
        stale_threshold_ms: Some(effective),
        change_view_reasons: snapshot
            .consensus_change_view_reasons
            .clone()
            .into_iter()
            .collect(),
        change_view_reason_counts: snapshot
            .consensus_change_view_reason_counts
            .clone(),
        change_view_total: snapshot.consensus_change_view_total,
    })
}

async fn wallet_accounts_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
) -> Json<Vec<Hash160>> {
    let hashes = wallet.read().await.script_hashes();
    Json(hashes)
}

#[derive(Deserialize)]
struct ImportWifRequest {
    wif: String,
    password: String,
    #[serde(default)]
    make_default: bool,
}

#[derive(Deserialize)]
struct ImportNep2Request {
    nep2: String,
    passphrase: String,
    password: String,
    #[serde(default)]
    make_default: bool,
    #[serde(default)]
    n: Option<u64>,
    #[serde(default)]
    r: Option<u32>,
    #[serde(default)]
    p: Option<u32>,
    #[serde(default = "default_address_version_byte")]
    address_version: u8,
}

const fn default_address_version_byte() -> u8 {
    DEFAULT_ADDRESS_VERSION.0
}

async fn wallet_import_wif_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    Json(payload): Json<ImportWifRequest>,
) -> Result<Json<Hash160>, (StatusCode, String)> {
    let mut wallet = wallet.write().await;
    let hash = wallet
        .import_wif(&payload.wif, &payload.password, payload.make_default)
        .map_err(internal_wallet_error)?;
    Ok(Json(hash))
}

async fn wallet_import_nep2_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    Json(payload): Json<ImportNep2Request>,
) -> Result<Json<Hash160>, (StatusCode, String)> {
    let mut wallet = wallet.write().await;
    let scrypt = ScryptParams {
        n: payload.n.unwrap_or(DEFAULT_SCRYPT_PARAMS.n),
        r: payload.r.unwrap_or(DEFAULT_SCRYPT_PARAMS.r),
        p: payload.p.unwrap_or(DEFAULT_SCRYPT_PARAMS.p),
    };
    let address_version = AddressVersion::new(payload.address_version);
    let hash = wallet
        .import_nep2(
            &payload.nep2,
            &payload.passphrase,
            &payload.password,
            scrypt,
            address_version,
            payload.make_default,
        )
        .map_err(internal_wallet_error)?;
    Ok(Json(hash))
}

#[derive(Deserialize)]
struct AccountsDetailRequest {
    password: String,
}

#[derive(Serialize, Deserialize)]
struct AccountDetailResponse {
    script_hash: String,
    label: Option<String>,
    is_default: bool,
    lock: bool,
    scopes: String,
    allowed_contracts: Vec<String>,
    allowed_groups: Vec<String>,
}

async fn wallet_accounts_detail_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    Json(payload): Json<AccountsDetailRequest>,
) -> Result<Json<Vec<AccountDetailResponse>>, (StatusCode, String)> {
    let details = wallet
        .write()
        .await
        .account_details(&payload.password)
        .map_err(internal_wallet_error)?;
    let response = details
        .into_iter()
        .map(|detail| AccountDetailResponse {
            script_hash: format!("{}", detail.script_hash),
            label: detail.label,
            is_default: detail.is_default,
            lock: detail.lock,
            scopes: detail.scopes.to_witness_scope_string(),
            allowed_contracts: detail
                .allowed_contracts
                .into_iter()
                .map(|hash| format!("{}", hash))
                .collect(),
            allowed_groups: detail
                .allowed_groups
                .into_iter()
                .map(|group| format!("0x{}", hex::encode(group)))
                .collect(),
        })
        .collect();
    Ok(Json(response))
}

#[derive(Deserialize)]
struct ExportWifRequest {
    script_hash: String,
    password: String,
}

#[derive(Deserialize)]
struct ExportNep2Request {
    script_hash: String,
    password: String,
    passphrase: String,
    #[serde(default)]
    n: Option<u64>,
    #[serde(default)]
    r: Option<u32>,
    #[serde(default)]
    p: Option<u32>,
    #[serde(default = "default_address_version_byte")]
    address_version: u8,
}

async fn wallet_export_wif_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    Json(payload): Json<ExportWifRequest>,
) -> Result<Json<String>, (StatusCode, String)> {
    let hash = parse_hash160(&payload.script_hash)?;
    let wallet = wallet.write().await;
    let wif = wallet
        .export_wif(&hash, &payload.password)
        .map_err(internal_wallet_error)?;
    Ok(Json(wif))
}

async fn wallet_export_nep2_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    Json(payload): Json<ExportNep2Request>,
) -> Result<Json<String>, (StatusCode, String)> {
    let hash = parse_hash160(&payload.script_hash)?;
    let scrypt = ScryptParams {
        n: payload.n.unwrap_or(DEFAULT_SCRYPT_PARAMS.n),
        r: payload.r.unwrap_or(DEFAULT_SCRYPT_PARAMS.r),
        p: payload.p.unwrap_or(DEFAULT_SCRYPT_PARAMS.p),
    };
    let address_version = AddressVersion::new(payload.address_version);
    let wallet = wallet.write().await;
    let nep2 = wallet
        .export_nep2(
            &hash,
            &payload.password,
            &payload.passphrase,
            scrypt,
            address_version,
        )
        .map_err(internal_wallet_error)?;
    Ok(Json(nep2))
}

#[derive(Deserialize)]
struct UpdateSignerRequest {
    script_hash: String,
    password: String,
    scopes: String,
    #[serde(default)]
    allowed_contracts: Vec<String>,
    #[serde(default)]
    allowed_groups: Vec<String>,
}

async fn wallet_update_signer_handler(
    wallet: Arc<RwLock<WalletStorage<DynStore>>>,
    Json(payload): Json<UpdateSignerRequest>,
) -> Result<Json<()>, (StatusCode, String)> {
    let hash = parse_hash160(&payload.script_hash)?;
    let scopes = SignerScopes::from_witness_scope_string(&payload.scopes)
        .ok_or((StatusCode::BAD_REQUEST, "invalid scopes".to_string()))?;
    let allowed_contracts = payload
        .allowed_contracts
        .iter()
        .map(|value| parse_hash160(value))
        .collect::<Result<Vec<_>, _>>()?;
    let allowed_groups = payload
        .allowed_groups
        .iter()
        .map(|value| {
            let trimmed = value.strip_prefix("0x").unwrap_or(value);
            hex::decode(trimmed).map_err(|_| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("invalid allowed group: {value}"),
                )
            })
            .and_then(|bytes| {
                if bytes.len() != 33 {
                    Err((
                        StatusCode::BAD_REQUEST,
                        format!("invalid allowed group length: {value}"),
                    ))
                } else {
                    Ok(bytes)
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    wallet
        .write()
        .await
        .update_signer_metadata(
            &hash,
            &payload.password,
            scopes,
            allowed_contracts,
            allowed_groups,
        )
        .map_err(internal_wallet_error)?;

    Ok(Json(()))
}

fn internal_wallet_error(err: WalletError) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, err.to_string())
}

fn parse_hash160(value: &str) -> Result<Hash160, (StatusCode, String)> {
    Hash160::from_str(value).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid script hash: {value}"),
        )
    })
}

async fn wallet_pending_handler(runtime: Arc<RwLock<Runtime>>) -> Json<Vec<String>> {
    let runtime = runtime.read().await;
    let ids = runtime.pending_ids().cloned().collect::<Vec<_>>();
    Json(ids)
}

async fn run_background_tasks(
    state: Arc<RwLock<NodeStatus>>,
    consensus: Arc<RwLock<DbftEngine>>,
    store: SharedStore,
    snapshot_key: SnapshotKey,
    runtime: Arc<RwLock<Runtime>>,
    stale_after_ms: u128,
) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let (
            height,
            view,
            participation,
            tallies,
            quorum,
            primary,
            validators,
            missing,
            expected,
            change_view_reason_counts,
            change_view_reasons,
            change_view_total,
            stages,
            stats,
        ) = {
            let mut engine = consensus.write().await;
            let next_height = engine.state().height() + 1;
            engine
                .advance_height(next_height)
                .map_err(anyhow::Error::from)
                .context("advance consensus height")?;
            persist_engine(store.as_ref(), snapshot_key, &engine)
                .map_err(anyhow::Error::from)
                .context("persist consensus snapshot")?;

            let mut runtime_guard = runtime.write().await;
            // Queue a couple of synthetic transactions to keep the mempool active.
            let next_id = format!("tick-{}", next_height);
            runtime_guard.queue_transaction(PendingTransaction::new(next_id, 10, 256));
            let spill_id = format!("tick-extra-{}", next_height);
            runtime_guard.queue_transaction(PendingTransaction::new(spill_id, 5, 128));
            let reserved = runtime_guard.tx_pool_mut().reserve_for_block(64, 64 * 1024);
            let fees = reserved.iter().map(|tx| tx.fee).sum();
            runtime_guard.commit_block(BlockSummary::new(
                engine.state().height(),
                reserved.len(),
                fees,
            ));

            let participation = engine.participation();
            let tallies = engine.tallies();
            let quorum = engine.quorum_threshold();
            let primary = engine.primary();
            let validators = describe_validators(engine.state().validators());
            let (missing, expected, stages) = gather_consensus_metrics(&engine, &participation);
            let change_view_reason_counts = engine.change_view_reason_counts();
            let change_view_reasons = engine.change_view_reasons();
            let change_view_total = engine.change_view_total();
            let stats = runtime_guard.stats();
            (
                engine.state().height(),
                engine.state().view().0,
                participation,
                tallies,
                quorum,
                primary,
                validators,
                missing,
                expected,
                change_view_reason_counts,
                change_view_reasons,
                change_view_total,
                stages,
                stats,
            )
        };

        let mut guard = state.write().await;
        guard.apply_consensus(
            height,
            ViewNumber(view),
            participation,
            tallies,
            quorum,
            primary,
            validators,
            missing,
            expected,
            change_view_reason_counts,
            change_view_reasons,
            change_view_total,
            stages,
            stale_after_ms,
        );
        guard.apply_runtime(&stats);
    }
}

fn default_validators() -> ValidatorSet {
    let mut validators = Vec::new();
    for idx in 0u16..4 {
        let mut bytes = [0u8; 32];
        bytes[31] = (idx + 1) as u8;
        let private = PrivateKey::new(bytes);
        let keypair = Keypair::from_private(private).expect("valid keypair");
        validators.push(Validator {
            id: ValidatorId(idx),
            public_key: keypair.public_key,
            alias: Some(format!("validator-{idx}")),
        });
    }
    ValidatorSet::new(validators)
}

fn configured_validators(entries: &[ValidatorConfig]) -> Result<ValidatorSet> {
    let mut validators = Vec::new();
    for entry in entries {
        let key_bytes = decode_public_key(&entry.public_key)
            .with_context(|| format!("decode public key for validator {}", entry.id))?;
        let public_key = PublicKey::from_sec1_bytes(&key_bytes)
            .map_err(|_| anyhow!("invalid sec1 public key for validator {}", entry.id))?;
        validators.push(Validator {
            id: ValidatorId(entry.id),
            public_key,
            alias: entry.alias.clone(),
        });
    }
    Ok(ValidatorSet::new(validators))
}

fn describe_validators(set: &ValidatorSet) -> Vec<ValidatorDescriptor> {
    set.iter()
        .map(|validator| ValidatorDescriptor {
            id: validator.id.0,
            public_key: validator.public_key.to_compressed().to_hex_lower(),
            script_hash: validator.public_key.script_hash().to_string(),
            alias: validator.alias.clone(),
        })
        .collect()
}

fn decode_public_key(input: &str) -> Result<Vec<u8>> {
    let trimmed = input.trim();
    let without_prefix = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    let bytes = decode(without_prefix).map_err(|err| anyhow!("invalid hex public key: {err}"))?;
    if bytes.len() != 33 && bytes.len() != 65 {
        return Err(anyhow!(
            "public key must be 33-byte compressed or 65-byte uncompressed"
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use chrono::{Duration, Utc};
    use neo_base::encoding::WifEncode;
    use neo_consensus::{persist_engine, ConsensusColumn, ConsensusState, MessageKind};
    use neo_crypto::{ecc256::PrivateKey, nep2::encrypt_nep2, scrypt::ScryptParams};
    use neo_runtime::Runtime;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tempfile::tempdir;
    use tower::ServiceExt;

    #[tokio::test]
    async fn status_endpoint_returns_snapshot() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let wallet = empty_wallet();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let response = app
            .oneshot(Request::get("/status").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let parsed: NodeStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(parsed.network, "testnet");
    }

    #[tokio::test]
    async fn loads_consensus_snapshot_if_present() {
        let memory_store = MemoryStore::new();
        memory_store.create_column(ConsensusColumn::ID);
        memory_store.create_column(ColumnId::new("wallet.keystore"));
        let store: SharedStore = Arc::new(memory_store);
        let validators = default_validators();
        let state = ConsensusState::new(5, ViewNumber(2), validators.clone());
        let engine = DbftEngine::new(state);
        let key = SnapshotKey { network: 42 };
        persist_engine(store.as_ref(), key, &engine).unwrap();

        let config = NodeConfig {
            network: "testnet".into(),
            network_magic: 42,
            rpc_bind: ([127, 0, 0, 1], 0).into(),
            snapshot_path: PathBuf::from("ignored"),
            stage_stale_after_ms: DEFAULT_STAGE_STALE_AFTER_MS,
            validators: None,
        };

        let node = Node::with_store(config, store).unwrap();
        let status = node.state.read().await;
        assert_eq!(status.height, 5);
        assert_eq!(status.view, 2);
    }

    #[tokio::test]
    async fn configured_validators_are_respected() {
        let base_set = default_validators();
        let configs = base_set
            .iter()
            .map(|validator| ValidatorConfig {
                id: validator.id.0,
                public_key: validator.public_key.to_compressed().to_hex_lower(),
                alias: Some(format!("alias-{}", validator.id.0)),
            })
            .collect::<Vec<_>>();

        let memory_store = MemoryStore::new();
        memory_store.create_column(ConsensusColumn::ID);
        memory_store.create_column(ColumnId::new("wallet.keystore"));
        let store: SharedStore = Arc::new(memory_store);

        let mut config = NodeConfig::default();
        config.validators = Some(configs.clone());

        let node =
            Node::with_store(config, store).expect("node initializes with config validators");
        let status = node.state.read().await;

        assert_eq!(status.consensus_validators.len(), configs.len());
        for descriptor in &status.consensus_validators {
            let expected = configs
                .iter()
                .find(|entry| entry.id == descriptor.id)
                .expect("validator present");
            assert_eq!(descriptor.alias, expected.alias);
            assert_eq!(descriptor.public_key, expected.public_key);
        }
    }

    #[tokio::test]
    async fn node_new_initializes_sled_store() {
        let dir = tempdir().unwrap();
        let snapshot_path = dir.path().join("consensus");
        let config = NodeConfig {
            network: "testnet".into(),
            network_magic: 123,
            rpc_bind: ([127, 0, 0, 1], 0).into(),
            snapshot_path: snapshot_path.clone(),
            stage_stale_after_ms: DEFAULT_STAGE_STALE_AFTER_MS,
            validators: None,
        };

        let node = Node::new(config).expect("node initializes Sled store");
        assert!(snapshot_path.exists());
        drop(node);
        dir.close().unwrap();
    }

    #[tokio::test]
    async fn wallet_accounts_endpoint_returns_hashes() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let (wallet, expected) = wallet_with_account();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(
            state.clone(),
            wallet.clone(),
            runtime.clone(),
            DEFAULT_STAGE_STALE_AFTER_MS,
        );

        let response = app
            .oneshot(
                Request::get("/wallet/accounts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let hashes: Vec<Hash160> = serde_json::from_slice(&body).unwrap();
        assert_eq!(hashes, vec![expected]);

        // pending endpoint returns queued runtime transactions
        {
            runtime
                .write()
                .await
                .queue_transaction(PendingTransaction::new("tx-test", 1, 10));
        }
        let pending_app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);
        let response = pending_app
            .oneshot(Request::get("/wallet/pending").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let pending: Vec<String> = serde_json::from_slice(&body).unwrap();
        assert!(pending.contains(&"tx-test".to_string()));
    }

    #[tokio::test]
    async fn wallet_import_wif_endpoint_imports_account() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let wallet = empty_wallet();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet.clone(), runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({
            "wif": "L3tgppXLgdaeqSGSFw1Go3skBiy8vQAM7YMXvTHsKQtE16PBncSU",
            "password": "pass",
            "make_default": true
        });

        let response = app
            .oneshot(
                Request::post("/wallet/import/wif")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let imported: Hash160 = serde_json::from_slice(&body).unwrap();
        let hashes = wallet.read().await.script_hashes();
        assert_eq!(hashes, vec![imported]);
    }

    #[tokio::test]
    async fn wallet_import_nep2_endpoint_imports_account() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let wallet = empty_wallet();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet.clone(), runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({
            "nep2": "6PYRzCDe46gkaR1E9AX3GyhLgQehypFvLG2KknbYjeNHQ3MZR2iqg8mcN3",
            "passphrase": "Satoshi",
            "password": "pass",
            "make_default": false,
            "n": 2,
            "r": 1,
            "p": 1,
            "address_version": DEFAULT_ADDRESS_VERSION.0,
        });

        let response = app
            .oneshot(
                Request::post("/wallet/import/nep2")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let imported: Hash160 = serde_json::from_slice(&body).unwrap();
        let hashes = wallet.read().await.script_hashes();
        assert_eq!(hashes, vec![imported]);
    }

    #[tokio::test]
    async fn wallet_accounts_detail_endpoint_returns_metadata() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let (wallet, hash) = wallet_with_account();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({ "password": "pass" });
        let response = app
            .oneshot(
                Request::post("/wallet/accounts/detail")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let accounts: Vec<AccountDetailResponse> = serde_json::from_slice(&body).unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].script_hash, format!("{}", hash));
    }

    #[tokio::test]
    async fn wallet_export_wif_endpoint_returns_wif() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let (wallet, hash) = wallet_with_account();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({
            "script_hash": hash.to_string(),
            "password": "pass"
        });

        let response = app
            .oneshot(
                Request::post("/wallet/export/wif")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let wif: String = serde_json::from_slice(&body).unwrap();
        let expected = PrivateKey::new([1u8; 32])
            .as_be_bytes()
            .wif_encode(0x80, true);
        assert_eq!(wif, expected);
    }

    #[tokio::test]
    async fn wallet_export_nep2_endpoint_returns_nep2() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let (wallet, hash) = wallet_with_account();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({
            "script_hash": hash.to_string(),
            "password": "pass",
            "passphrase": "Satoshi",
            "n": 2,
            "r": 1,
            "p": 1,
            "address_version": DEFAULT_ADDRESS_VERSION.0
        });

        let response = app
            .oneshot(
                Request::post("/wallet/export/nep2")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let nep2: String = serde_json::from_slice(&body).unwrap();
        let expected = encrypt_nep2(
            &PrivateKey::new([1u8; 32]),
            "Satoshi",
            DEFAULT_ADDRESS_VERSION,
            ScryptParams { n: 2, r: 1, p: 1 },
        )
        .expect("encrypt nep2");
        assert_eq!(nep2, expected);
    }

    #[tokio::test]
    async fn wallet_update_signer_endpoint_applies_metadata() {
        let (wallet, hash) = wallet_with_account();
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet.clone(), runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({
            "script_hash": hash.to_string(),
            "password": "pass",
            "scopes": "CustomContracts",
            "allowed_contracts": [hash.to_string()],
            "allowed_groups": [],
        });

        let response = app
            .oneshot(
                Request::post("/wallet/update/signer")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let details = wallet
            .read()
            .await
            .account_details("pass")
            .expect("account details");
        let metadata = details
            .into_iter()
            .find(|detail| detail.script_hash == hash)
            .expect("account metadata");
        assert_eq!(metadata.scopes, SignerScopes::CUSTOM_CONTRACTS);
        assert_eq!(metadata.allowed_contracts, vec![hash]);
        assert!(metadata.allowed_groups.is_empty());
    }

    #[tokio::test]
    async fn wallet_update_signer_endpoint_rejects_inconsistent_input() {
        let (wallet, hash) = wallet_with_account();
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let payload = json!({
            "script_hash": hash.to_string(),
            "password": "pass",
            "scopes": "CalledByEntry",
            "allowed_contracts": [hash.to_string()],
            "allowed_groups": [],
        });

        let response = app
            .oneshot(
                Request::post("/wallet/update/signer")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn consensus_endpoint_reports_metrics() {
        let state = Arc::new(RwLock::new(NodeStatus::new("testnet".into())));
        let expected_validators;
        {
            let mut guard = state.write().await;
            guard.height = 7;
            guard.view = 3;
            guard.consensus_quorum = 5;
            guard.consensus_primary = Some(0);
            guard.consensus_validators = describe_validators(&default_validators());
            guard.consensus_tallies.insert("PrepareResponse".into(), 3);
            guard
                .consensus_participation
                .insert("PrepareResponse".into(), vec![1, 2, 3]);
            guard
                .consensus_missing
                .insert("PrepareResponse".into(), vec![4, 5]);
            guard.consensus_expected.insert("PrepareResponse".into(), 4);
            guard
                .consensus_change_view_reasons
                .insert(2, ChangeViewReason::Timeout);
            guard
                .consensus_change_view_reason_counts
                .insert("Timeout".into(), 1);
            guard.consensus_change_view_total = 1;
            guard.consensus_stale_threshold_ms = Some(DEFAULT_STAGE_STALE_AFTER_MS);
            guard.consensus_stages.insert(
                "PrepareResponse".into(),
                StageStatus {
                    state: StageState::Pending,
                    expected: Some(4),
                    responded: 3,
                    missing: vec![4, 5],
                    last_updated: Utc::now(),
                    age_ms: 0,
                    stale: false,
                },
            );
            expected_validators = guard.consensus_validators.clone();
        }
        let wallet = empty_wallet();
        let runtime = Arc::new(RwLock::new(Runtime::new(100, 1)));
        let app = super::build_router(state, wallet, runtime, DEFAULT_STAGE_STALE_AFTER_MS);

        let response = app
            .oneshot(Request::get("/consensus").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), axum::http::StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let stats: ConsensusStatus = serde_json::from_slice(&body).unwrap();
        assert_eq!(stats.height, 7);
        assert_eq!(stats.view, 3);
        assert_eq!(stats.quorum, 5);
        assert_eq!(stats.primary, Some(0));
        assert_eq!(stats.validators, expected_validators);
        assert_eq!(stats.tallies.get("PrepareResponse"), Some(&3usize));
        assert_eq!(
            stats.participation.get("PrepareResponse"),
            Some(&vec![1, 2, 3])
        );
        assert_eq!(stats.missing.get("PrepareResponse"), Some(&vec![4, 5]));
        assert_eq!(stats.expected.get("PrepareResponse"), Some(&4usize));
        assert_eq!(stats.change_view_reason_counts.get("Timeout"), Some(&1usize));
        assert_eq!(
            stats.change_view_reasons.get(&2),
            Some(&ChangeViewReason::Timeout)
        );
        assert_eq!(stats.change_view_total, 1);
        let stage = stats.stages.get("PrepareResponse").unwrap();
        assert_eq!(stage.state, StageState::Pending);
        assert_eq!(stage.expected, Some(4));
        assert_eq!(stage.responded, 3);
        assert_eq!(stage.missing, vec![4, 5]);
        assert!(stage.last_updated <= Utc::now());
        assert!(!stage.stale);
        assert_eq!(stats.stale_threshold_ms, Some(DEFAULT_STAGE_STALE_AFTER_MS));
    }

    #[tokio::test]
    async fn stage_marked_stale_when_age_exceeds_threshold() {
        let mut status = NodeStatus::new("testnet".into());
        let mut stages = BTreeMap::new();
        stages.insert(
            MessageKind::Commit,
            StageStatus {
                state: StageState::Pending,
                expected: Some(4),
                responded: 2,
                missing: vec![1, 2],
                last_updated: Utc::now() - Duration::milliseconds(6_000),
                age_ms: 0,
                stale: false,
            },
        );
        status.apply_consensus(
            10,
            ViewNumber::ZERO,
            BTreeMap::new(),
            BTreeMap::new(),
            3,
            None,
            Vec::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            0,
            stages,
            5_000,
        );
        let stage = status
            .consensus_stages
            .get("Commit")
            .expect("stage present");
        assert!(stage.stale);
        assert!(stage.age_ms >= 5_000);
        assert_eq!(status.consensus_stale_threshold_ms, Some(5_000));

        let override_query = ConsensusQuery {
            stale_threshold_ms: Some(10_000),
        };
        let Json(response) = consensus_handler(
            Arc::new(RwLock::new(status)),
            axum::extract::Query(override_query),
            5_000,
        )
        .await;
        assert_eq!(response.stale_threshold_ms, Some(10_000));
    }

    #[test]
    fn status_tracks_primary_expectation_on_bootstrap() {
        let validators = default_validators();
        let state = ConsensusState::new(0, ViewNumber::ZERO, validators.clone());
        let engine = DbftEngine::new(state);
        let runtime = Runtime::new(100, 1);

        let status = NodeStatus::from_state(
            "mainnet".into(),
            &engine,
            &runtime,
            DEFAULT_STAGE_STALE_AFTER_MS,
        );
        let primary = validators
            .primary_id(0, ViewNumber::ZERO)
            .expect("primary exists");
        let expected_validators = describe_validators(&validators);

        assert_eq!(
            status.consensus_expected.get("PrepareRequest"),
            Some(&1usize)
        );
        assert_eq!(status.consensus_primary, Some(primary.0));
        assert_eq!(
            status.consensus_missing.get("PrepareRequest"),
            Some(&vec![primary.0])
        );
        assert_eq!(status.consensus_validators, expected_validators);
        let stage = status
            .consensus_stages
            .get("PrepareRequest")
            .expect("stage present");
        assert_eq!(stage.expected, Some(1));
        assert_eq!(stage.responded, 0);
        assert_eq!(stage.missing, vec![primary.0]);
    }

    fn empty_wallet() -> Arc<RwLock<WalletStorage<DynStore>>> {
        wallet_from_store(None).0
    }

    fn wallet_with_account() -> (Arc<RwLock<WalletStorage<DynStore>>>, Hash160) {
        let (wallet, hash) = wallet_from_store(Some(PrivateKey::new([1u8; 32])));
        (wallet, hash.expect("script hash"))
    }

    fn wallet_from_store(
        private: Option<PrivateKey>,
    ) -> (Arc<RwLock<WalletStorage<DynStore>>>, Option<Hash160>) {
        let raw_store = Arc::new(MemoryStore::new());
        let column = ColumnId::new("wallet.keystore");
        raw_store.create_column(column);
        let shared: SharedStore = raw_store.clone();
        let mut storage = WalletStorage::<DynStore>::open(shared, column, b"default".to_vec())
            .expect("wallet storage open");
        let hash = if let Some(private) = private {
            let account = storage
                .import_private_key(private, "pass")
                .expect("import account");
            Some(account.script_hash())
        } else {
            None
        };
        (Arc::new(RwLock::new(storage)), hash)
    }
}
