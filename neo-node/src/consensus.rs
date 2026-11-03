//! dBFT consensus wiring for neo-node.

use crate::ai_agent::AiAgentRuntime;
use crate::config::DbftSettings;
use crate::signing_provider::{
    CONSENSUS_SIGNING_PRINCIPAL, NEO_SIGN_DATA_ENCODING, NodeSignerRuntime,
};
use neo_ai_agent::{EvidenceKind, EvidenceSource};
use neo_consensus::{
    BlockData, ChangeViewReason, CommitPersistencePermit, ConsensusContext, ConsensusEvent,
    ConsensusMessageType, ConsensusPayload, ConsensusService, ConsensusSigner, ValidatorInfo,
};
use neo_core::ScriptBuilder;
use neo_core::cryptography::MerkleTree;
use neo_core::i_event_handlers::MessageReceivedHandler;
use neo_core::ledger::{
    PersistCompleted, RelayResult, TransactionVerificationContext, VerifyResult,
};
use neo_core::neo_io::MemoryReader;
use neo_core::network::p2p::local_node::RelayInventory;
use neo_core::network::p2p::payloads::{
    Block, ExtensiblePayload, Header, InvPayload, InventoryType, Transaction, TransactionAttribute,
    Witness,
};
use neo_core::network::p2p::{
    Message, MessageCommand, MessageHandlerSubscription, register_message_received_handler,
};
use neo_core::persistence::Store;
use neo_core::prelude::Serializable;
use neo_core::runtime::{Actor, ActorContext, ActorRef, ActorResult, Props, ScheduleHandle};
use neo_core::signing::{
    FrozenSignIntent, ProtocolContext, SIGNING_SCHEMA_VERSION, SignPayload, SignPurpose,
};
use neo_core::smart_contract::ContractParametersContext;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::{LedgerContract, NeoToken, helpers::NativeHelpers};
use neo_core::time_provider::TimeProvider;
use neo_core::wallets::Wallet;
use neo_core::{ContainsTransactionType, UInt160, UInt256};
use neo_crypto::Crypto;
use neo_vm::OpCode;
use parking_lot::Mutex;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, warn};

const CONSENSUS_STATE_KEY: [u8; 1] = [0xf4];

fn authorize_commit_egress(
    permit: Option<&CommitPersistencePermit>,
    block_index: u32,
    view_number: u8,
    validator_index: u8,
    signature: &[u8],
    persist: impl FnOnce(&CommitPersistencePermit) -> bool,
) -> bool {
    let Some(permit) = permit else {
        return false;
    };
    permit.covers_commit(block_index, view_number, validator_index, signature) && persist(permit)
}

fn authorize_consensus_payload_egress(
    payload: &ConsensusPayload,
    my_index: Option<u8>,
    persist: impl FnOnce(&CommitPersistencePermit) -> bool,
) -> bool {
    match payload.message_type {
        ConsensusMessageType::Commit if my_index == Some(payload.validator_index) => {
            authorize_commit_egress(
                payload.commit_persistence.as_ref(),
                payload.block_index,
                payload.view_number,
                payload.validator_index,
                &payload.data,
                persist,
            )
        }
        ConsensusMessageType::RecoveryMessage => {
            let Ok(recovery) = neo_consensus::messages::RecoveryMessage::deserialize(
                &payload.data,
                payload.block_index,
                payload.view_number,
                payload.validator_index,
            ) else {
                return false;
            };
            let Some(local_commit) = my_index.and_then(|local_index| {
                recovery
                    .commit_messages
                    .iter()
                    .find(|commit| commit.validator_index == local_index)
            }) else {
                return true;
            };
            authorize_commit_egress(
                payload.commit_persistence.as_ref(),
                payload.block_index,
                local_commit.view_number,
                local_commit.validator_index,
                &local_commit.signature,
                persist,
            )
        }
        _ => true,
    }
}

fn authorize_block_commit_egress(
    block_data: &BlockData,
    my_index: Option<u8>,
    persist: impl FnOnce(&CommitPersistencePermit) -> bool,
) -> bool {
    let Some(local_index) = my_index else {
        return true;
    };
    let Some((_, signature)) = block_data
        .signatures
        .iter()
        .find(|(validator_index, _)| *validator_index == local_index)
    else {
        return true;
    };
    let view_number = block_data
        .commit_persistence
        .as_ref()
        .map_or(0, |permit| permit.view_number);
    authorize_commit_egress(
        block_data.commit_persistence.as_ref(),
        block_data.block_index,
        view_number,
        local_index,
        signature,
        persist,
    )
}

/// Wallet-changed handler that starts/stops the consensus actor.
pub struct DbftConsensusController {
    system: Arc<neo_core::neo_system::NeoSystem>,
    settings: DbftSettings,
    signer_runtime: Arc<NodeSignerRuntime>,
    diagnostics: Arc<AiAgentRuntime>,
    actor: Mutex<Option<ActorRef>>,
    _message_subscription: MessageHandlerSubscription,
}

impl DbftConsensusController {
    pub fn new(
        system: Arc<neo_core::neo_system::NeoSystem>,
        settings: DbftSettings,
        signer_runtime: Arc<NodeSignerRuntime>,
        diagnostics: Arc<AiAgentRuntime>,
    ) -> Self {
        let filter = Arc::new(DbftMessageFilter::new(settings.max_block_system_fee));
        let subscription = register_message_received_handler(filter);
        install_mempool_filter(&system, settings.max_block_system_fee);
        Self {
            system,
            settings,
            signer_runtime,
            diagnostics,
            actor: Mutex::new(None),
            _message_subscription: subscription,
        }
    }

    pub fn start_with_wallet(&self, wallet: Arc<dyn Wallet>) -> bool {
        let mut guard = self.actor.lock();
        if guard.is_some() {
            return false;
        }

        let props = ConsensusActor::props(
            self.system.clone(),
            self.settings.clone(),
            wallet.clone(),
            Arc::clone(&self.signer_runtime),
            Arc::clone(&self.diagnostics),
        );
        match self.system.actor_system().actor_of(props, "dbft-consensus") {
            Ok(actor) => {
                if !self.settings.auto_start {
                    let _ = actor.tell(ConsensusActorMessage::ManualStart);
                }
                *guard = Some(actor);
                true
            }
            Err(err) => {
                warn!(target: "neo", %err, "failed to start dBFT consensus actor");
                false
            }
        }
    }

    pub fn stop(&self) -> bool {
        let mut guard = self.actor.lock();
        if let Some(actor) = guard.take() {
            if let Err(err) = self.system.actor_system().stop(&actor) {
                warn!(target: "neo", %err, "failed to stop dBFT consensus actor");
                return false;
            }
            return true;
        }
        false
    }

    pub fn is_running(&self) -> bool {
        self.actor.lock().is_some()
    }
}

impl neo_core::i_event_handlers::WalletChangedHandler for DbftConsensusController {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn Wallet>>,
    ) {
        if !self.settings.auto_start {
            return;
        }

        self.stop();
        if let Some(wallet) = wallet {
            self.start_with_wallet(wallet);
        }
    }
}

struct DbftMessageFilter {
    max_block_system_fee: i64,
}

impl DbftMessageFilter {
    fn new(max_block_system_fee: i64) -> Self {
        Self {
            max_block_system_fee,
        }
    }
}

impl MessageReceivedHandler for DbftMessageFilter {
    fn remote_node_message_received_handler(&self, _system: &dyn Any, message: &Message) -> bool {
        if message.command != MessageCommand::Transaction {
            return true;
        }

        let mut reader = MemoryReader::new(&message.payload_raw);
        let Ok(tx) = Transaction::deserialize(&mut reader) else {
            return true;
        };

        tx.system_fee() <= self.max_block_system_fee
    }
}

fn install_mempool_filter(
    system: &Arc<neo_core::neo_system::NeoSystem>,
    max_block_system_fee: i64,
) {
    let mempool = system.mempool();
    let mut guard = mempool.lock();
    let existing = guard.new_transaction.take();
    guard.new_transaction = Some(Box::new(move |pool, args| {
        if let Some(handler) = &existing {
            handler(pool, args);
        }
        if !args.cancel && args.transaction.system_fee() > max_block_system_fee {
            args.cancel = true;
        }
    }));
}

struct WalletConsensusSigner {
    wallet: Arc<dyn Wallet>,
    runtime: Arc<NodeSignerRuntime>,
    signer: neo_core::signing::TrustedSignerClient,
    network: u32,
}

impl WalletConsensusSigner {
    fn new(
        wallet: Arc<dyn Wallet>,
        runtime: Arc<NodeSignerRuntime>,
        network: u32,
    ) -> neo_core::signing::SignResult<Self> {
        let signer = runtime.trusted_client(
            CONSENSUS_SIGNING_PRINCIPAL,
            "node-local-consensus",
            "sign:consensus",
        )?;
        Ok(Self {
            wallet,
            runtime,
            signer,
            network,
        })
    }
}

impl ConsensusSigner for WalletConsensusSigner {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.wallet
            .get_account(script_hash)
            .is_some_and(|account| !account.is_locked() && account.has_key())
    }

    fn sign(
        &self,
        data: &[u8],
        script_hash: &UInt160,
        context: &neo_consensus::ConsensusSignContext,
    ) -> neo_consensus::ConsensusResult<Vec<u8>> {
        let payload_digest = UInt256::from(Crypto::sha256(data));
        let proposal_digest = context
            .proposal_digest
            .map(|digest| digest.to_string())
            .unwrap_or_else(|| "none".to_string());
        let operation_id = format!(
            "consensus:{}:{}:{}:{}:{}:{}:{}",
            self.network,
            context.block_index,
            context.view_number,
            context.message_type.to_byte(),
            script_hash,
            payload_digest,
            u8::from(context.commit_block_hash),
        );
        let key_ref = self
            .runtime
            .resolve_key(&script_hash.to_string(), &SignPurpose::Consensus)
            .map_err(|error| {
                neo_consensus::ConsensusError::state_error(format!(
                    "Consensus signer key resolution failed: {error}"
                ))
            })?;
        let intent = FrozenSignIntent {
            schema_version: SIGNING_SCHEMA_VERSION,
            network: self.network,
            purpose: SignPurpose::Consensus,
            key_ref,
            payload: SignPayload::Canonical {
                encoding: NEO_SIGN_DATA_ENCODING.to_string(),
                bytes: data.to_vec(),
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-dbft".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::from([
                    ("block_index".to_string(), context.block_index.to_string()),
                    ("view_number".to_string(), context.view_number.to_string()),
                    ("message_type".to_string(), context.message_type.to_string()),
                    ("proposal_digest".to_string(), proposal_digest),
                    (
                        "commit_block_hash".to_string(),
                        context.commit_block_hash.to_string(),
                    ),
                    ("signer_account".to_string(), script_hash.to_string()),
                ]),
            },
            policy_version: self.runtime.policy_version().to_string(),
            fee_units: 0,
            quota_units: 1,
            account: Some(script_hash.to_string()),
            targets: BTreeSet::new(),
        };
        let signer = self.signer.clone();
        let future = async move { signer.resume_or_start(&operation_id, intent).await };
        let result = if let Ok(handle) = tokio::runtime::Handle::try_current()
            && handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread
        {
            tokio::task::block_in_place(|| handle.block_on(future))
                .map_err(|error| error.to_string())
        } else {
            match std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| error.to_string())?
                    .block_on(future)
                    .map_err(|error| error.to_string())
            })
            .join()
            {
                Ok(result) => result,
                Err(_) => Err("Signer service thread panicked".to_string()),
            }
        };
        result.map(|response| response.signature).map_err(|error| {
            neo_consensus::ConsensusError::state_error(format!(
                "Consensus signer service failed: {error}"
            ))
        })
    }
}

#[derive(Debug, Clone)]
enum ConsensusActorMessage {
    ServiceEvent(ConsensusEvent),
    TimerTick,
    ManualStart,
}

struct ConsensusActor {
    system: Arc<neo_core::neo_system::NeoSystem>,
    settings: DbftSettings,
    wallet: Arc<dyn Wallet>,
    signer_runtime: Arc<NodeSignerRuntime>,
    diagnostics: Arc<AiAgentRuntime>,
    service: Option<ConsensusService>,
    event_task: Option<JoinHandle<()>>,
    timer: Option<ScheduleHandle>,
    proposal_transactions: HashMap<UInt256, Transaction>,
    pending_block: Option<BlockData>,
    missing_transactions: Option<HashSet<UInt256>>,
    current_block_index: Option<u32>,
    current_prev_hash: Option<UInt256>,
    prev_timestamp: Option<u64>,
    validators: Vec<neo_core::cryptography::ECPoint>,
    validator_infos: Vec<ValidatorInfo>,
    my_index: Option<u8>,
    recovery_path: PathBuf,
    recovery_store: Option<Arc<dyn Store>>,
    recovery_requested: bool,
}

impl ConsensusActor {
    fn new(
        system: Arc<neo_core::neo_system::NeoSystem>,
        settings: DbftSettings,
        wallet: Arc<dyn Wallet>,
        signer_runtime: Arc<NodeSignerRuntime>,
        diagnostics: Arc<AiAgentRuntime>,
    ) -> Self {
        let recovery_path = resolve_recovery_path(&settings.recovery_logs);
        let recovery_store = open_recovery_store(&system, &settings);
        Self {
            system,
            settings,
            wallet,
            signer_runtime,
            diagnostics,
            service: None,
            event_task: None,
            timer: None,
            proposal_transactions: HashMap::new(),
            pending_block: None,
            missing_transactions: None,
            current_block_index: None,
            current_prev_hash: None,
            prev_timestamp: None,
            validators: Vec::new(),
            validator_infos: Vec::new(),
            my_index: None,
            recovery_path,
            recovery_store,
            recovery_requested: false,
        }
    }

    fn props(
        system: Arc<neo_core::neo_system::NeoSystem>,
        settings: DbftSettings,
        wallet: Arc<dyn Wallet>,
        signer_runtime: Arc<NodeSignerRuntime>,
        diagnostics: Arc<AiAgentRuntime>,
    ) -> Props {
        Props::new(move || {
            Self::new(
                system.clone(),
                settings.clone(),
                wallet.clone(),
                Arc::clone(&signer_runtime),
                Arc::clone(&diagnostics),
            )
        })
    }

    fn current_time_ms() -> u64 {
        let millis = TimeProvider::current().utc_now_timestamp_millis();
        if millis < 0 { 0 } else { millis as u64 }
    }

    fn start_timer(&mut self, ctx: &ActorContext) {
        // Fixed 1-second interval matching C# `new System.Timers.Timer(1000)`.
        // The previous `block_ms / 5` (3s on MainNet) delayed timeout detection
        // and view-change initiation, hurting consensus liveness.
        let interval = Duration::from_secs(1);
        self.timer = Some(ctx.schedule_repeatedly(
            interval,
            interval,
            &ctx.self_ref(),
            ConsensusActorMessage::TimerTick,
        ));
    }

    fn stop_timer(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }

    fn reset_round_state(&mut self) {
        self.proposal_transactions.clear();
        self.pending_block = None;
        self.missing_transactions = None;
    }

    fn export_consensus_snapshot(&self, event: &'static str) {
        let Some(service) = self.service.as_ref() else {
            return;
        };
        let context = service.context();
        let phase = if !context.commits.is_empty() {
            "commit"
        } else if context.proposed_block_hash.is_some() {
            "prepare"
        } else {
            "proposal"
        };
        let fields = BTreeMap::from([
            ("event".to_string(), serde_json::json!(event)),
            ("view".to_string(), serde_json::json!(context.view_number)),
            ("phase".to_string(), serde_json::json!(phase)),
            (
                "role".to_string(),
                serde_json::json!(if context.is_primary() {
                    "primary"
                } else {
                    "backup"
                }),
            ),
            (
                "prepare_votes".to_string(),
                serde_json::json!(context.prepare_responses.len()),
            ),
            (
                "commit_votes".to_string(),
                serde_json::json!(context.commits.len()),
            ),
            (
                "missing_transactions".to_string(),
                serde_json::json!(self.missing_transactions.as_ref().map_or(0, HashSet::len)),
            ),
            (
                "recovery_active".to_string(),
                serde_json::json!(service.is_recovering()),
            ),
            (
                "signing_authority_available".to_string(),
                serde_json::json!(self.my_index.is_some()),
            ),
            (
                "active_validators".to_string(),
                serde_json::json!(self.validators.len()),
            ),
            (
                "expected_validators".to_string(),
                serde_json::json!(self.validators.len()),
            ),
        ]);
        let _ = self.diagnostics.try_export_consensus_snapshot(fields);
    }

    fn load_validators(&self, next_index: u32) -> Vec<neo_core::cryptography::ECPoint> {
        let store_cache = self.system.store_cache();
        let snapshot = store_cache.data_cache();
        let settings = self.system.settings();
        let validators_count = settings.validators_count.max(0) as usize;
        let refresh =
            NativeHelpers::should_refresh_committee(next_index, settings.committee_members_count());

        let result = if refresh {
            NeoToken::new().compute_next_block_validators_snapshot(snapshot, settings)
        } else {
            NeoToken::new().get_next_block_validators_snapshot(snapshot, validators_count, settings)
        };

        result.unwrap_or_else(|_| settings.standby_validators())
    }

    fn build_validator_infos(validators: &[neo_core::cryptography::ECPoint]) -> Vec<ValidatorInfo> {
        validators
            .iter()
            .enumerate()
            .map(|(idx, pubkey)| ValidatorInfo {
                index: idx as u8,
                public_key: pubkey.clone(),
                script_hash: Contract::create_signature_contract(pubkey.clone()).script_hash(),
            })
            .collect()
    }

    fn find_signing_key(&self, validators: &[neo_core::cryptography::ECPoint]) -> Option<u8> {
        for (idx, pubkey) in validators.iter().enumerate() {
            let contract = Contract::create_signature_contract(pubkey.clone());
            let script_hash = contract.script_hash();
            let Some(account) = self.wallet.get_account(&script_hash) else {
                continue;
            };
            if !account.is_locked() && account.has_key() {
                return Some(idx as u8);
            }
        }
        None
    }

    fn install_service(
        &mut self,
        service: ConsensusService,
        mut event_rx: mpsc::Receiver<ConsensusEvent>,
        ctx: &ActorContext,
    ) {
        if let Some(handle) = self.event_task.take() {
            handle.abort();
        }

        let actor_ref = ctx.self_ref();
        let handle = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                let _ = actor_ref.tell(ConsensusActorMessage::ServiceEvent(event));
            }
        });

        self.event_task = Some(handle);
        self.service = Some(service);
    }

    fn start_round(
        &mut self,
        block_index: u32,
        prev_hash: UInt256,
        prev_timestamp: u64,
        ctx: &ActorContext,
    ) {
        self.reset_round_state();
        self.current_block_index = Some(block_index);
        self.current_prev_hash = Some(prev_hash);
        self.prev_timestamp = Some(prev_timestamp);

        let validators = self.load_validators(block_index);
        let validator_infos = Self::build_validator_infos(&validators);
        let my_index = self.find_signing_key(&validators);
        let network = self.system.settings().network;
        let signer = match WalletConsensusSigner::new(
            self.wallet.clone(),
            Arc::clone(&self.signer_runtime),
            network,
        ) {
            Ok(signer) => Arc::new(signer),
            Err(error) => {
                warn!(
                    target: "neo",
                    %error,
                    "consensus signer client initialization failed; consensus disabled"
                );
                self.service = None;
                if let Some(handle) = self.event_task.take() {
                    handle.abort();
                }
                return;
            }
        };

        self.validators = validators;
        self.validator_infos = validator_infos.clone();
        self.my_index = my_index;

        let Some(my_index) = my_index else {
            warn!(
                target: "neo",
                "dBFT wallet has no signable validator account; consensus disabled"
            );
            self.service = None;
            if let Some(handle) = self.event_task.take() {
                handle.abort();
            }
            return;
        };
        let expected_block_time = self.system.time_per_block().as_millis() as u64;
        let max_transactions_per_block = self.system.settings().max_transactions_per_block as usize;
        let now = Self::current_time_ms();

        let mut use_recovery = None;
        if !self.settings.ignore_recovery_logs {
            if self.recovery_store.is_some() {
                if let Some(context) =
                    self.load_recovery_from_store(validator_infos.clone(), Some(my_index))
                {
                    if context.block_index == block_index {
                        use_recovery = Some(context);
                    } else {
                        warn!(target: "neo", "recovery log does not match current height; ignoring");
                    }
                }
            } else {
                let fallback_path = if self.recovery_path.is_dir() {
                    self.recovery_path.join("consensus.context")
                } else {
                    self.recovery_path.clone()
                };
                if fallback_path.is_file() {
                    match ConsensusContext::load(
                        &fallback_path,
                        validator_infos.clone(),
                        Some(my_index),
                    ) {
                        Ok(context) if context.block_index == block_index => {
                            use_recovery = Some(context);
                        }
                        Ok(_) => {
                            warn!(
                                target: "neo",
                                "recovery log does not match current height; ignoring"
                            );
                        }
                        Err(err) => {
                            warn!(target: "neo", %err, "failed to load consensus recovery log");
                        }
                    }
                }
            }
        }

        if let Some(context) = use_recovery {
            let recovered_transactions_empty = context.proposed_tx_hashes.is_empty();
            let (event_tx, event_rx) = mpsc::channel(256);
            let mut service =
                ConsensusService::with_context(network, context, Vec::new(), event_tx);
            service.set_signer(Some(signer.clone()));
            service.set_expected_block_time(expected_block_time);
            service.set_max_transactions_per_block(max_transactions_per_block);
            service.set_prev_timestamp(prev_timestamp);
            if let Err(err) = service.resume(now, prev_hash, 0) {
                warn!(target: "neo", %err, "failed to resume consensus from recovery log");
            }
            self.install_service(service, event_rx, ctx);
            if recovered_transactions_empty {
                if let Some(service) = self.service.as_mut() {
                    let _ = service.on_transactions_received(Vec::new());
                }
            } else {
                self.try_prepare_response(now);
            }
            return;
        }

        if let Some(service) = self.service.as_mut() {
            service.update_validators(validator_infos, Some(my_index));
            service.set_signer(Some(signer.clone()));
            service.set_expected_block_time(expected_block_time);
            service.set_max_transactions_per_block(max_transactions_per_block);
            service.set_prev_timestamp(prev_timestamp);
            if let Err(err) = service.start(block_index, now, prev_hash, 0) {
                warn!(target: "neo", %err, "failed to start consensus round");
            }
            return;
        }

        let (event_tx, event_rx) = mpsc::channel(256);
        let mut service = ConsensusService::new(
            network,
            validator_infos,
            Some(my_index),
            Vec::new(),
            event_tx,
        );
        service.set_signer(Some(signer));
        service.set_expected_block_time(expected_block_time);
        service.set_max_transactions_per_block(max_transactions_per_block);
        service.set_prev_timestamp(prev_timestamp);
        if let Err(err) = service.start(block_index, now, prev_hash, 0) {
            warn!(target: "neo", %err, "failed to start consensus round");
        }
        self.install_service(service, event_rx, ctx);
    }

    fn start_from_chain(&mut self, ctx: &ActorContext) {
        let current_height = self.system.current_block_index();
        let prev_hash = self
            .system
            .block_hash_at(current_height)
            .unwrap_or_default();
        let prev_timestamp = fetch_block_timestamp(&self.system, current_height).unwrap_or(0);
        self.start_round(current_height + 1, prev_hash, prev_timestamp, ctx);
        self.request_recovery_on_start();
    }

    fn request_recovery_on_start(&mut self) {
        if self.recovery_requested {
            return;
        }

        let Some(service) = self.service.as_mut() else {
            return;
        };

        let commit_sent = service
            .context()
            .my_index
            .and_then(|idx| service.context().commits.get(&idx))
            .is_some();
        if commit_sent {
            self.recovery_requested = true;
            return;
        }

        if let Err(err) = service.request_recovery() {
            warn!(target: "neo", %err, "failed to request consensus recovery");
        }
        self.recovery_requested = true;
    }

    fn on_persist_completed(&mut self, mut block: Block, ctx: &ActorContext) {
        let next_index = block.index().saturating_add(1);
        let hash = block.hash();
        let _ = self.diagnostics.try_export(
            EvidenceKind::ChainCommit,
            EvidenceSource::NodeRuntime,
            block.index(),
            BTreeMap::from([
                ("persisted".to_string(), serde_json::json!(true)),
                (
                    "transaction_count".to_string(),
                    serde_json::json!(block.transactions.len()),
                ),
            ]),
        );
        // C# compares PrepareRequest.Timestamp against the parent header's
        // timestamp. Using local wall-clock time here rejects honest proposals
        // that were built before this node finished persisting the parent.
        let prev_timestamp = block.header.timestamp();
        self.start_round(next_index, hash, prev_timestamp, ctx);
    }

    fn on_timer_tick(&mut self) {
        let now = Self::current_time_ms();
        self.try_prepare_response(now);
        self.try_finalize_pending_block(now);

        let reason = if self
            .missing_transactions
            .as_ref()
            .map(|missing| !missing.is_empty())
            .unwrap_or(false)
        {
            ChangeViewReason::TxNotFound
        } else {
            ChangeViewReason::Timeout
        };
        let Some(service) = self.service.as_mut() else {
            return;
        };
        if let Err(err) = service.on_timer_tick_with_reason(now, reason) {
            warn!(target: "neo", %err, ?reason, "failed to process consensus timer");
        }
        self.export_consensus_snapshot("timer");
    }

    async fn on_service_event(&mut self, event: ConsensusEvent) {
        match event {
            ConsensusEvent::BroadcastMessage(payload) => {
                self.broadcast_consensus_message(payload).await;
            }
            ConsensusEvent::RequestTransactions { max_count, .. } => {
                self.propose_transactions(max_count);
            }
            ConsensusEvent::BlockCommitted { block_data, .. } => {
                if !self.authorize_block_commit(&block_data) {
                    return;
                }
                self.pending_block = Some(block_data);
                self.try_finalize_pending_block(Self::current_time_ms());
            }
            ConsensusEvent::ViewChanged { .. } => {
                self.reset_round_state();
                self.export_consensus_snapshot("view_changed");
            }
        }
    }

    async fn broadcast_consensus_message(&mut self, payload: ConsensusPayload) {
        if !self.authorize_consensus_payload(&payload) {
            return;
        }

        let Some(extensible) = self.build_extensible_payload(&payload) else {
            return;
        };

        if let Err(err) = self
            .system
            .local_node_handle()
            .send_directly(RelayInventory::Extensible(extensible), None)
        {
            warn!(target: "neo", %err, "failed to broadcast consensus payload");
        }
    }

    fn authorize_consensus_payload(&self, payload: &ConsensusPayload) -> bool {
        let authorized = authorize_consensus_payload_egress(payload, self.my_index, |permit| {
            self.persist_commit_permit(permit)
        });
        if !authorized {
            error!(target: "neo", validator = payload.validator_index, "local Commit persistence authorization failed; withholding broadcast");
        }
        authorized
    }

    fn authorize_block_commit(&self, block_data: &BlockData) -> bool {
        let authorized = authorize_block_commit_egress(block_data, self.my_index, |permit| {
            self.persist_commit_permit(permit)
        });
        if !authorized {
            error!(target: "neo", validator = ?self.my_index, "final block local Commit persistence authorization failed; withholding block");
        }
        authorized
    }

    fn build_extensible_payload(&self, payload: &ConsensusPayload) -> Option<ExtensiblePayload> {
        let validator = self
            .validators
            .get(payload.validator_index as usize)?
            .clone();
        let sender = Contract::create_signature_contract(validator.clone()).script_hash();
        let signature = payload.witness.clone();
        let mut builder = ScriptBuilder::new();
        builder.emit_push(&signature);
        let invocation = builder.to_array();
        let verification = Contract::create_signature_redeem_script(validator);
        let witness = Witness::new_with_scripts(invocation, verification);

        let mut extensible = ExtensiblePayload::new();
        extensible.category = "dBFT".to_string();
        extensible.valid_block_start = 0;
        extensible.valid_block_end = payload.block_index;
        extensible.sender = sender;
        extensible.data = payload.to_message_bytes();
        extensible.witness = witness;
        Some(extensible)
    }

    fn propose_transactions(&mut self, max_count: usize) {
        if self.service.is_none() {
            return;
        }

        let settings = self.system.settings();
        let max_tx = settings.max_transactions_per_block as usize;
        let limit = std::cmp::min(max_count, max_tx);

        let mut selected = Vec::new();
        let mut total_size = 0usize;
        let mut total_system_fee = 0i64;

        let required_signatures = consensus_m_threshold(self.validators.len());
        let header_size = estimate_header_size(&self.validators, required_signatures);

        let candidates = self.proposal_candidates(limit);
        let rejected: HashSet<UInt256> = self
            .service
            .as_ref()
            .map(|service| {
                service
                    .context()
                    .rejected_hashes_above_f()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        for tx in candidates {
            // Skip transactions already rejected by >F validators; re-proposing
            // them only burns views (C# InvalidTransactions).
            if rejected.contains(&tx.hash()) {
                continue;
            }
            let candidate_count = selected.len() + 1;
            let size_with_count =
                header_size + var_size(candidate_count as u64) + total_size + tx.size();
            if size_with_count > self.settings.max_block_size as usize {
                break;
            }

            let fee_with_tx = total_system_fee.saturating_add(tx.system_fee());
            if fee_with_tx > self.settings.max_block_system_fee {
                break;
            }

            total_size += tx.size();
            total_system_fee = fee_with_tx;
            selected.push(tx);
        }

        // Convert Arc<Transaction> to Transaction for proposal
        self.proposal_transactions = selected
            .iter()
            .map(|arc_tx| (arc_tx.hash(), (**arc_tx).clone()))
            .collect();

        let hashes = selected.into_iter().map(|tx| tx.hash()).collect();
        if let Some(service) = self.service.as_mut()
            && let Err(err) = service.on_transactions_received(hashes)
        {
            warn!(target: "neo", %err, "failed to submit proposal transactions");
        }
    }

    fn proposal_candidates(&mut self, limit: usize) -> Vec<Arc<Transaction>> {
        if limit == 0 {
            return Vec::new();
        }

        let pool = self.system.mempool();

        // Disabled diagnostics must retain the original proposal selection path
        // byte-for-byte in behavior, including fee ordering and limit handling.
        if !self.diagnostics.is_enabled() {
            return pool.lock().get_sorted_verified_transactions(limit);
        }

        #[cfg(feature = "tee")]
        if let Some(runtime) = crate::tee_integration::active_runtime() {
            // Obtain TEE ordering first, but intersect it with the bounded local-only
            // quarantine view. Network receive and external proposal paths never call this API.
            let tee_hashes = runtime
                .mempool
                .get_ordered_hashes(limit.saturating_mul(4).max(limit));
            let mut pool_guard = pool.lock();
            let locally_allowed = pool_guard.local_proposal_transactions(
                limit.saturating_mul(4).max(limit),
                Self::current_time_ms(),
            );

            let mut verified_by_hash = HashMap::new();
            for tx in &locally_allowed {
                verified_by_hash.insert(tx.hash(), Arc::clone(tx));
            }

            let mut candidates = Vec::with_capacity(limit);
            let mut seen = HashSet::with_capacity(limit.saturating_mul(2));

            for hash_bytes in tee_hashes {
                if candidates.len() >= limit {
                    break;
                }

                let Ok(hash) = UInt256::from_bytes(&hash_bytes) else {
                    continue;
                };
                let Some(tx) = verified_by_hash.get(&hash) else {
                    continue;
                };

                let tx_hash = tx.hash();
                if seen.insert(tx_hash) {
                    candidates.push(Arc::clone(tx));
                }
            }

            if candidates.len() < limit {
                for tx in locally_allowed {
                    let tx_hash = tx.hash();
                    if seen.insert(tx_hash) {
                        candidates.push(tx);
                    }
                    if candidates.len() >= limit {
                        break;
                    }
                }
            }

            return candidates;
        }

        pool.lock()
            .local_proposal_transactions(limit, Self::current_time_ms())
    }

    fn try_prepare_response(&mut self, now: u64) {
        let diagnostics = &self.diagnostics;
        let Some(service) = self.service.as_mut() else {
            return;
        };

        if service.context().is_primary() {
            return;
        }

        let Some(my_index) = self.my_index else {
            return;
        };

        if service.context().prepare_responses.contains_key(&my_index) {
            return;
        }

        if !service.context().prepare_request_received {
            return;
        }

        let proposed_hashes = service.context().proposed_tx_hashes.clone();
        if proposed_hashes.is_empty() {
            let _ = service.on_transactions_received(Vec::new());
            return;
        }

        let settings = self.system.settings();
        let store_cache = self.system.store_cache();
        let snapshot = store_cache.data_cache();
        let pool = self.system.mempool();
        let pool_guard = pool.lock();
        let verified_map: HashMap<UInt256, Transaction> = pool_guard
            .verified_transactions_vec()
            .into_iter()
            .map(|arc_tx| ((*arc_tx).clone().hash(), (*arc_tx).clone()))
            .collect();
        let unverified_map: HashMap<UInt256, Transaction> = pool_guard
            .unverified_transactions_vec()
            .into_iter()
            .map(|arc_tx| ((*arc_tx).clone().hash(), (*arc_tx).clone()))
            .collect();
        drop(pool_guard);

        let mut verification_context = TransactionVerificationContext::new();
        let mut transactions = HashMap::new();
        let mut missing = HashSet::new();

        for hash in &proposed_hashes {
            if self.system.context().contains_transaction(hash)
                == ContainsTransactionType::ExistsInLedger
            {
                warn!(
                    target: "neo",
                    tx_hash = %hash,
                    "prepare request includes on-chain transaction"
                );
                service.record_rejected_transaction(*hash);
                let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                return;
            }

            if let Some(tx) = verified_map.get(hash) {
                let signer_accounts = signer_accounts(tx);
                if self
                    .system
                    .context()
                    .contains_conflict_hash(hash, &signer_accounts)
                {
                    warn!(
                        target: "neo",
                        tx_hash = %hash,
                        "prepare request contains conflicting transaction"
                    );
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }
                if tx.system_fee() > self.settings.max_block_system_fee {
                    warn!(
                        target: "neo",
                        tx_hash = %hash,
                        "transaction system fee exceeds max_block_system_fee"
                    );
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }
                if conflicts_with_context(tx, &transactions) {
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }
                transactions.insert(*hash, tx.clone());
                verification_context.add_transaction(tx);
                continue;
            }

            if let Some(tx) = unverified_map.get(hash) {
                let signer_accounts = signer_accounts(tx);
                if self
                    .system
                    .context()
                    .contains_conflict_hash(hash, &signer_accounts)
                {
                    warn!(
                        target: "neo",
                        tx_hash = %hash,
                        "prepare request contains conflicting transaction"
                    );
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }

                if tx.system_fee() > self.settings.max_block_system_fee {
                    warn!(
                        target: "neo",
                        tx_hash = %hash,
                        "transaction system fee exceeds max_block_system_fee"
                    );
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }

                if conflicts_with_context(tx, &transactions) {
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }

                let result = tx.verify(settings, snapshot, Some(&verification_context), &[]);
                if result != VerifyResult::Succeed {
                    let _ = diagnostics.try_export(
                        EvidenceKind::TransactionValidation,
                        EvidenceSource::NodeRuntime,
                        service.context().block_index,
                        BTreeMap::from([
                            (
                                "transaction_hash".to_string(),
                                serde_json::json!(hash.to_string()),
                            ),
                            (
                                "validation_result".to_string(),
                                serde_json::json!(format!("{result:?}").to_ascii_lowercase()),
                            ),
                            ("deterministic".to_string(), serde_json::json!(true)),
                            ("path".to_string(), serde_json::json!("prepare_response")),
                        ]),
                    );
                    let reason = if result == VerifyResult::PolicyFail {
                        ChangeViewReason::TxRejectedByPolicy
                    } else {
                        ChangeViewReason::TxInvalid
                    };
                    service.record_rejected_transaction(*hash);
                    let _ = service.request_change_view(reason, now);
                    return;
                }

                transactions.insert(*hash, tx.clone());
                verification_context.add_transaction(tx);
                continue;
            }

            missing.insert(*hash);
        }

        if !missing.is_empty() {
            let should_request = match self.missing_transactions.as_ref() {
                Some(prev) => prev != &missing,
                None => true,
            };
            if should_request {
                let hashes: Vec<UInt256> = missing.iter().copied().collect();
                let inv = InvPayload::create(InventoryType::Transaction, &hashes);
                if let Err(err) = self
                    .system
                    .task_manager_handle()
                    .broadcast_restart_tasks(inv)
                {
                    debug!(target: "neo", %err, "failed to request missing transactions");
                }
                self.missing_transactions = Some(missing);
            }
            return;
        }

        let total_system_fee = transactions
            .values()
            .fold(0i64, |acc, tx| acc.saturating_add(tx.system_fee()));
        if total_system_fee > self.settings.max_block_system_fee {
            let _ = service.request_change_view(ChangeViewReason::BlockRejectedByPolicy, now);
            return;
        }

        let required_signatures = consensus_m_threshold(self.validators.len());
        let header_size = estimate_header_size(&self.validators, required_signatures);
        let total_tx_size = transactions.values().map(|tx| tx.size()).sum::<usize>();
        let block_size = header_size + var_size(transactions.len() as u64) + total_tx_size;
        if block_size > self.settings.max_block_size as usize {
            let _ = service.request_change_view(ChangeViewReason::BlockRejectedByPolicy, now);
            return;
        }

        self.proposal_transactions = transactions;
        self.missing_transactions = None;
        let _ = service.on_transactions_received(proposed_hashes);
    }

    fn try_finalize_pending_block(&mut self, _now: u64) {
        let Some(block_data) = self.pending_block.clone() else {
            return;
        };

        let current_index = self.current_block_index.unwrap_or_default();
        if block_data.block_index != current_index {
            self.pending_block = None;
            return;
        }

        let mut missing = Vec::new();
        let mut transactions = Vec::with_capacity(block_data.transaction_hashes.len());

        for hash in &block_data.transaction_hashes {
            if let Some(tx) = self.proposal_transactions.get(hash) {
                transactions.push(tx.clone());
                continue;
            }
            if let Some(tx) = self.system.context().try_get_transaction_from_mempool(hash) {
                transactions.push(tx);
                continue;
            }
            missing.push(*hash);
        }

        if !missing.is_empty() {
            let inv = InvPayload::create(InventoryType::Transaction, &missing);
            if let Err(err) = self
                .system
                .task_manager_handle()
                .broadcast_restart_tasks(inv)
            {
                debug!(target: "neo", %err, "failed to request block transactions");
            }
            return;
        }

        let Some(prev_hash) = self.current_prev_hash else {
            warn!(target: "neo", "missing prev_hash for consensus block");
            return;
        };

        let merkle_root =
            MerkleTree::compute_root(&block_data.transaction_hashes).unwrap_or_default();
        let next_consensus = NativeHelpers::get_bft_address(&block_data.validator_pubkeys);

        let mut header = Header::new();
        header.set_version(0);
        header.set_prev_hash(prev_hash);
        header.set_merkle_root(merkle_root);
        header.set_timestamp(block_data.timestamp);
        header.set_nonce(block_data.nonce);
        header.set_index(block_data.block_index);
        header.set_primary_index(block_data.primary_index);
        header.set_next_consensus(next_consensus);

        let store_cache = self.system.store_cache();
        let snapshot = Arc::new(store_cache.data_cache().clone());
        let contract = Contract::create_multi_sig_contract(
            block_data.required_signatures,
            &block_data.validator_pubkeys,
        );
        let mut context = ContractParametersContext::new(
            snapshot,
            header.clone(),
            self.system.settings().network,
        );
        for (validator_index, signature) in &block_data.signatures {
            if let Some(pubkey) = block_data.validator_pubkeys.get(*validator_index as usize) {
                let _ = context.add_signature(contract.clone(), pubkey.clone(), signature.clone());
            }
        }

        let Some(witnesses) = context.witnesses() else {
            warn!(target: "neo", "failed to build block witness from commits");
            return;
        };
        header.witness = witnesses.into_iter().next().unwrap_or_else(Witness::new);

        let mut block = Block {
            header,
            transactions,
        };
        block.rebuild_merkle_root();

        if let Err(err) = self.system.blockchain_actor().tell(
            neo_core::ledger::BlockchainCommand::InventoryBlock {
                block: std::sync::Arc::new(block),
                relay: true,
                pre_verified: false,
            },
        ) {
            warn!(target: "neo", %err, "failed to submit consensus block");
            return;
        }

        self.pending_block = None;
    }

    fn on_relay_result(&mut self, result: RelayResult) {
        if result.result != VerifyResult::Succeed {
            return;
        }

        if result.inventory_type == InventoryType::Transaction {
            let waiting_on_tx = self
                .missing_transactions
                .as_ref()
                .is_some_and(|missing| missing.contains(&result.hash))
                || self
                    .pending_block
                    .as_ref()
                    .is_some_and(|block| block.transaction_hashes.contains(&result.hash));
            if waiting_on_tx {
                let now = Self::current_time_ms();
                self.try_prepare_response(now);
                self.try_finalize_pending_block(now);
            }
            return;
        }

        if result.inventory_type != InventoryType::Extensible {
            return;
        }

        let context = self.system.context();
        let payload = context
            .try_get_relay_extensible(&result.hash)
            .or_else(|| context.try_get_extensible(&result.hash));
        let Some(payload) = payload else {
            return;
        };

        if payload.category != "dBFT" || payload.data.is_empty() {
            return;
        }

        let Some(signature) = extract_signature(&payload.witness) else {
            return;
        };

        let network = self.system.settings().network;
        let consensus_payload =
            match ConsensusPayload::from_message_bytes(network, &payload.data, signature) {
                Ok(value) => value,
                Err(err) => {
                    debug!(target: "neo", %err, "failed to parse consensus payload");
                    return;
                }
            };

        if !self.precheck_prepare_request(&consensus_payload) {
            return;
        }

        let Some(service) = self.service.as_mut() else {
            return;
        };

        if let Err(err) = service.process_message(consensus_payload) {
            debug!(target: "neo", %err, "consensus message rejected");
            return;
        }

        self.try_prepare_response(Self::current_time_ms());
    }

    fn precheck_prepare_request(&self, payload: &ConsensusPayload) -> bool {
        if payload.message_type != ConsensusMessageType::PrepareRequest {
            return true;
        }

        let Some(current_index) = self.current_block_index else {
            return false;
        };

        if payload.block_index != current_index {
            return false;
        }

        let Ok(message) = neo_consensus::messages::PrepareRequestMessage::deserialize_body(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        ) else {
            return false;
        };

        if message.transaction_hashes.len()
            > self.system.settings().max_transactions_per_block as usize
        {
            return false;
        }

        if let Some(prev_hash) = self.current_prev_hash
            && message.prev_hash != prev_hash
        {
            return false;
        }

        let prev_timestamp = self.prev_timestamp.unwrap_or(0);
        if message.timestamp <= prev_timestamp {
            return false;
        }

        let now = Self::current_time_ms();
        let max_future = self.system.time_per_block().as_millis() as u64 * 8;
        if message.timestamp > now.saturating_add(max_future) {
            return false;
        }

        true
    }

    fn load_recovery_from_store(
        &self,
        validator_infos: Vec<ValidatorInfo>,
        my_index: Option<u8>,
    ) -> Option<ConsensusContext> {
        let store = self.recovery_store.as_ref()?;
        let snapshot = store.snapshot();
        let key = CONSENSUS_STATE_KEY.to_vec();
        let data = snapshot.try_get(&key)?;
        match ConsensusContext::from_bytes(&data, validator_infos, my_index) {
            Ok(context) => Some(context),
            Err(err) => {
                warn!(target: "neo", %err, "failed to decode consensus recovery log");
                None
            }
        }
    }

    fn persist_commit_permit(&self, permit: &CommitPersistencePermit) -> bool {
        if permit.validate().is_err() {
            return false;
        }

        if let Some(store) = self.recovery_store.as_ref() {
            let mut snapshot = store.snapshot();
            if let Some(snap) = Arc::get_mut(&mut snapshot)
                && snap
                    .put_sync(CONSENSUS_STATE_KEY.to_vec(), permit.recovery_state.clone())
                    .is_ok()
                && snap.try_commit_durable().is_ok()
                && store
                    .snapshot()
                    .try_get(&CONSENSUS_STATE_KEY.to_vec())
                    .as_deref()
                    == Some(permit.recovery_state.as_slice())
            {
                return true;
            }
            error!(target: "neo", "durable consensus recovery store write failed; withholding signature egress");
            return false;
        }

        let fallback_path = if self.recovery_path.is_dir() {
            self.recovery_path.join("consensus.context")
        } else {
            self.recovery_path.clone()
        };
        match ConsensusContext::save_bytes_durable(&fallback_path, &permit.recovery_state) {
            Ok(()) => true,
            Err(err) => {
                error!(target: "neo", %err, "commit recovery state could not be durably persisted; withholding signature egress");
                false
            }
        }
    }
}

#[async_trait::async_trait]
impl Actor for ConsensusActor {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let stream = self.system.event_stream();
        stream.subscribe::<PersistCompleted>(ctx.self_ref());
        stream.subscribe::<RelayResult>(ctx.self_ref());
        self.start_timer(ctx);
        if self.settings.auto_start {
            self.start_from_chain(ctx);
        }
        Ok(())
    }

    async fn post_stop(&mut self, ctx: &mut ActorContext) -> ActorResult {
        self.stop_timer();
        self.system.event_stream().unsubscribe_all(&ctx.self_ref());
        if let Some(handle) = self.event_task.take() {
            handle.abort();
        }
        Ok(())
    }

    async fn handle(
        &mut self,
        envelope: Box<dyn std::any::Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        let envelope = match envelope.downcast::<PersistCompleted>() {
            Ok(message) => {
                let block =
                    std::sync::Arc::try_unwrap(message.block).unwrap_or_else(|arc| (*arc).clone());
                self.on_persist_completed(block, _ctx);
                return Ok(());
            }
            Err(envelope) => envelope,
        };

        let envelope = match envelope.downcast::<RelayResult>() {
            Ok(message) => {
                self.on_relay_result(*message);
                return Ok(());
            }
            Err(envelope) => envelope,
        };

        if let Ok(message) = envelope.downcast::<ConsensusActorMessage>() {
            match *message {
                ConsensusActorMessage::ServiceEvent(event) => self.on_service_event(event).await,
                ConsensusActorMessage::TimerTick => self.on_timer_tick(),
                ConsensusActorMessage::ManualStart => {
                    if self.service.is_none() {
                        self.start_from_chain(_ctx);
                    }
                }
            }
        }

        Ok(())
    }
}

fn resolve_recovery_path(value: &str) -> PathBuf {
    // Return the path as-is - PathBuf handles both absolute and relative paths
    PathBuf::from(value)
}

fn open_recovery_store(
    system: &neo_core::neo_system::NeoSystem,
    settings: &DbftSettings,
) -> Option<Arc<dyn Store>> {
    if settings.ignore_recovery_logs {
        return None;
    }
    let path = settings.recovery_logs.trim();
    if path.is_empty() {
        return None;
    }
    match system.store_provider().get_store(path) {
        Ok(store) if store.supports_durable_writes() => Some(store),
        Ok(_) => {
            warn!(target: "neo", "consensus recovery store lacks durable writes; using durable file mode");
            None
        }
        Err(err) => {
            warn!(target: "neo", %err, "failed to open consensus recovery store; using durable file mode");
            None
        }
    }
}

fn fetch_block_timestamp(system: &neo_core::neo_system::NeoSystem, index: u32) -> Option<u64> {
    let store_cache = system.store_cache();
    let ledger = LedgerContract::new();
    ledger
        .get_block(&store_cache, HashOrIndex::Index(index))
        .ok()
        .flatten()
        .map(|block| block.header.timestamp)
}

fn extract_signature(witness: &Witness) -> Option<Vec<u8>> {
    let invocation = &witness.invocation_script;
    if invocation.len() != 66 {
        return None;
    }
    if invocation[0] != OpCode::PUSHDATA1.byte() || invocation[1] != 0x40 {
        return None;
    }
    Some(invocation[2..66].to_vec())
}

fn consensus_m_threshold(n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let f = (n - 1) / 3;
    n - f
}

fn estimate_header_size(validators: &[neo_core::cryptography::ECPoint], m: usize) -> usize {
    if validators.is_empty() || m == 0 {
        return 0;
    }

    let dummy_sig = vec![0u8; 64];
    let mut builder = ScriptBuilder::new();
    for _ in 0..m {
        builder.emit_push(&dummy_sig);
    }
    let invocation = builder.to_array();
    let verification = Contract::create_multi_sig_redeem_script(m, validators);
    let witness = Witness::new_with_scripts(invocation, verification);
    let mut header = Header::new();
    header.witness = witness;
    header.size()
}

fn var_size(count: u64) -> usize {
    neo_core::neo_io::serializable::helper::get_var_size(count)
}

fn conflicts_with_context(tx: &Transaction, existing: &HashMap<UInt256, Transaction>) -> bool {
    let new_conflicts = conflict_hashes(tx);
    if new_conflicts.iter().any(|hash| existing.contains_key(hash)) {
        warn!(target: "neo", tx_hash = %tx.hash(), "transaction conflicts with existing set");
        return true;
    }

    for other in existing.values() {
        if conflict_hashes(other).iter().any(|hash| *hash == tx.hash()) {
            warn!(target: "neo", tx_hash = %tx.hash(), "existing transaction conflicts with new tx");
            return true;
        }
    }
    false
}

fn conflict_hashes(tx: &Transaction) -> Vec<UInt256> {
    tx.attributes()
        .iter()
        .filter_map(|attr| match attr {
            TransactionAttribute::Conflicts(conflicts) => Some(conflicts.hash),
            _ => None,
        })
        .collect()
}

fn signer_accounts(tx: &Transaction) -> Vec<neo_core::UInt160> {
    tx.signers().iter().map(|signer| signer.account).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use neo_core::protocol_settings::ProtocolSettings;
    use neo_core::signing::{
        AuthenticatedContext, FrozenSignIntent, IdempotencyInfo, KeyRef, SignError, SignErrorCode,
        SignRequest, SignResponse, SignResult, SignerService, SigningKeyBinding,
        SigningKeyResolver,
    };
    use neo_core::wallets::{KeyPair, Nep6Wallet};
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct RecordingConsensusSigner {
        expected_key_ref: KeyRef,
        requests: StdMutex<Vec<(SignRequest, AuthenticatedContext)>>,
        resume_calls: AtomicUsize,
        pending_first: bool,
    }

    impl RecordingConsensusSigner {
        fn record_and_sign(
            &self,
            request: SignRequest,
            context: AuthenticatedContext,
        ) -> SignResult<SignResponse> {
            let request_digest = request.binding_digest(&context)?;
            self.requests
                .lock()
                .expect("consensus recording lock")
                .push((request.clone(), context));
            if request.key_ref != self.expected_key_ref {
                return Err(SignError::new(
                    SignErrorCode::KeyNotFound,
                    "consensus signer key provider, id, or version is not registered",
                ));
            }
            Ok(SignResponse {
                schema_version: SIGNING_SCHEMA_VERSION,
                request_id: request.request_id.clone(),
                key_ref: request.key_ref.clone(),
                algorithm: neo_core::signing::ALGORITHM_ECDSA_P256_SHA256.to_string(),
                signature: vec![0x5A; 64],
                signed_digest: request.payload.signing_digest(),
                request_digest,
                policy_version: request.policy_version.clone(),
                completed_at_unix_ms: 1,
                idempotent_replay: false,
            })
        }
    }

    #[async_trait]
    impl SignerService for RecordingConsensusSigner {
        async fn sign(
            &self,
            request: SignRequest,
            context: AuthenticatedContext,
        ) -> SignResult<SignResponse> {
            self.record_and_sign(request, context)
        }

        async fn resume_or_start(
            &self,
            operation_id: &str,
            frozen_intent: FrozenSignIntent,
            context: AuthenticatedContext,
            request_timeout: Duration,
        ) -> SignResult<SignResponse> {
            let call = self.resume_calls.fetch_add(1, Ordering::SeqCst);
            let request = frozen_intent.into_request(
                operation_id,
                request_timeout.as_millis().min(i64::MAX as u128) as i64,
            )?;
            if self.pending_first && call == 0 {
                self.requests
                    .lock()
                    .expect("consensus recording lock")
                    .push((request.clone(), context));
                return Err(SignError::new(
                    SignErrorCode::ResultUnknown,
                    "recorded consensus signing operation remains Pending",
                )
                .for_request(request.request_id)
                .retryable(true));
            }
            self.record_and_sign(request, context)
        }
    }

    fn valid_commit_permit() -> CommitPersistencePermit {
        let validators = vec![ValidatorInfo {
            index: 0,
            public_key: neo_crypto::ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
            script_hash: UInt160::zero(),
        }];
        let mut context = ConsensusContext::new(42, validators, Some(0), None);
        let proposal_hash = UInt256::from([0x42; 32]);
        context.proposed_block_hash = Some(proposal_hash);
        context
            .add_commit(0, 0, vec![0xA5; 64])
            .expect("add local commit");
        context
            .commit_persistence_permit()
            .expect("build commit permit")
            .expect("local commit permit")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn consensus_uses_exact_configured_non_default_sign_request() {
        let settings = Arc::new(ProtocolSettings::default());
        let wallet = Arc::new(Nep6Wallet::new(
            Some("consensus-routing".to_string()),
            None,
            settings,
        ));
        let key_pair = KeyPair::from_private_key(&[0x31; 32]).expect("consensus key");
        let account = key_pair.get_script_hash();
        wallet
            .create_account(&[0x31; 32])
            .await
            .expect("wallet account");
        let wallet_trait: Arc<dyn Wallet> = wallet;
        let configured = KeyRef::new("consensus-hsm", "validator-key-v4", 4);
        let resolver = Arc::new(
            SigningKeyResolver::new([SigningKeyBinding {
                account: account.to_string(),
                purpose: SignPurpose::Consensus,
                key_ref: configured.clone(),
                public_key: key_pair.public_key(),
            }])
            .expect("consensus key resolver"),
        );
        let service = Arc::new(RecordingConsensusSigner {
            expected_key_ref: configured.clone(),
            requests: StdMutex::new(Vec::new()),
            resume_calls: AtomicUsize::new(0),
            pending_first: false,
        });
        let runtime = Arc::new(NodeSignerRuntime::for_test(
            service.clone(),
            "software-default",
            "consensus-policy-v3",
            Duration::from_secs(5),
            resolver,
        ));
        let network = 860_833_102;
        let signer =
            WalletConsensusSigner::new(wallet_trait, runtime, network).expect("consensus signer");
        let context = neo_consensus::ConsensusSignContext {
            block_index: 42,
            view_number: 3,
            message_type: ConsensusMessageType::Commit,
            proposal_digest: Some(UInt256::from([0x42; 32])),
            commit_block_hash: true,
        };
        let data = [1, 2, 3, 4];

        let signature = signer
            .sign(&data, &account, &context)
            .expect("consensus signature");
        assert_eq!(signature, vec![0x5A; 64]);
        let requests = service.requests.lock().expect("consensus recording lock");
        assert_eq!(requests.len(), 1);
        let payload_digest = UInt256::from(Crypto::sha256(&data));
        let operation_id = format!(
            "consensus:{network}:42:3:{}:{account}:{payload_digest}:1",
            ConsensusMessageType::Commit.to_byte()
        );
        let expected_request = SignRequest {
            schema_version: SIGNING_SCHEMA_VERSION,
            request_id: operation_id.clone(),
            network,
            purpose: SignPurpose::Consensus,
            key_ref: configured,
            payload: SignPayload::Canonical {
                encoding: NEO_SIGN_DATA_ENCODING.to_string(),
                bytes: data.to_vec(),
            },
            protocol_context: ProtocolContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                protocol: "neo-n3-dbft".to_string(),
                protocol_version: "1".to_string(),
                fields: BTreeMap::from([
                    ("block_index".to_string(), "42".to_string()),
                    ("view_number".to_string(), "3".to_string()),
                    ("message_type".to_string(), "Commit".to_string()),
                    (
                        "proposal_digest".to_string(),
                        UInt256::from([0x42; 32]).to_string(),
                    ),
                    ("commit_block_hash".to_string(), "true".to_string()),
                    ("signer_account".to_string(), account.to_string()),
                ]),
            },
            policy_version: "consensus-policy-v3".to_string(),
            deadline_unix_ms: 5_000,
            idempotency: IdempotencyInfo {
                schema_version: SIGNING_SCHEMA_VERSION,
                key: operation_id.clone(),
                replay_nonce: operation_id,
            },
            fee_units: 0,
            quota_units: 1,
            account: Some(account.to_string()),
            targets: BTreeSet::new(),
        };
        assert_eq!(requests[0].0, expected_request);
        assert_eq!(requests[0].1.principal, CONSENSUS_SIGNING_PRINCIPAL);
        assert_eq!(
            requests[0].1.scopes,
            BTreeSet::from(["sign:consensus".to_string()])
        );
        drop(requests);
        assert_eq!(service.resume_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn consensus_rejects_unknown_provider_and_version_without_default_fallback() {
        let settings = Arc::new(ProtocolSettings::default());
        let wallet = Arc::new(Nep6Wallet::new(
            Some("consensus-key-mismatch".to_string()),
            None,
            settings,
        ));
        let key_pair = KeyPair::from_private_key(&[0x32; 32]).expect("consensus key");
        let account = key_pair.get_script_hash();
        wallet
            .create_account(&[0x32; 32])
            .await
            .expect("wallet account");
        let configured = KeyRef::new("consensus-hsm", "validator-key-v4", 4);
        let context = neo_consensus::ConsensusSignContext {
            block_index: 43,
            view_number: 1,
            message_type: ConsensusMessageType::Commit,
            proposal_digest: Some(UInt256::from([0x43; 32])),
            commit_block_hash: true,
        };

        for rejected in [
            KeyRef::new("unknown-consensus-provider", "validator-key-v4", 4),
            KeyRef::new("consensus-hsm", "validator-key-v4", 5),
        ] {
            let resolver = Arc::new(
                SigningKeyResolver::new([SigningKeyBinding {
                    account: account.to_string(),
                    purpose: SignPurpose::Consensus,
                    key_ref: rejected.clone(),
                    public_key: key_pair.public_key(),
                }])
                .expect("rejected consensus key resolver"),
            );
            let service = Arc::new(RecordingConsensusSigner {
                expected_key_ref: configured.clone(),
                requests: StdMutex::new(Vec::new()),
                resume_calls: AtomicUsize::new(0),
                pending_first: false,
            });
            let runtime = Arc::new(NodeSignerRuntime::for_test(
                service.clone(),
                "software-default",
                "consensus-policy-v3",
                Duration::from_secs(5),
                resolver,
            ));
            let signer = WalletConsensusSigner::new(wallet.clone(), runtime, 860_833_102)
                .expect("consensus signer");

            signer
                .sign(&[4, 3, 2, 1], &account, &context)
                .expect_err("unknown provider or version must fail closed");
            let requests = service.requests.lock().expect("consensus recording lock");
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].0.key_ref, rejected);
            assert_ne!(requests[0].0.key_ref.provider, "software-default");
            assert_eq!(requests[0].0.purpose, SignPurpose::Consensus);
            assert_eq!(
                requests[0].1.scopes,
                BTreeSet::from(["sign:consensus".to_string()])
            );
            assert_eq!(service.resume_calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn recovered_pending_signature_without_permit_has_no_consensus_egress() {
        let settings = Arc::new(ProtocolSettings::default());
        let wallet = Arc::new(Nep6Wallet::new(
            Some("consensus-pending-recovery".to_string()),
            None,
            settings,
        ));
        let key_pair = KeyPair::from_private_key(&[0x33; 32]).expect("consensus key");
        let account = key_pair.get_script_hash();
        wallet
            .create_account(&[0x33; 32])
            .await
            .expect("wallet account");
        let configured = KeyRef::new("consensus-hsm", "validator-pending", 7);
        let resolver = Arc::new(
            SigningKeyResolver::new([SigningKeyBinding {
                account: account.to_string(),
                purpose: SignPurpose::Consensus,
                key_ref: configured.clone(),
                public_key: key_pair.public_key(),
            }])
            .expect("consensus key resolver"),
        );
        let service = Arc::new(RecordingConsensusSigner {
            expected_key_ref: configured,
            requests: StdMutex::new(Vec::new()),
            resume_calls: AtomicUsize::new(0),
            pending_first: true,
        });
        let runtime = Arc::new(NodeSignerRuntime::for_test(
            service.clone(),
            "software-default",
            "consensus-policy-v3",
            Duration::from_secs(5),
            resolver,
        ));
        let network = 860_833_102;
        let signer =
            WalletConsensusSigner::new(wallet, runtime, network).expect("consensus signer");
        let context = neo_consensus::ConsensusSignContext {
            block_index: 42,
            view_number: 3,
            message_type: ConsensusMessageType::Commit,
            proposal_digest: Some(UInt256::from([0x42; 32])),
            commit_block_hash: true,
        };
        let sign_data = [9, 8, 7, 6];

        signer
            .sign(&sign_data, &account, &context)
            .expect_err("first dispatch remains Pending");
        let recovered_signature = signer
            .sign(&sign_data, &account, &context)
            .expect("Pending operation recovers its signature");
        assert_eq!(recovered_signature, vec![0x5A; 64]);
        let requests = service.requests.lock().expect("consensus recording lock");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0], requests[1]);
        drop(requests);
        assert_eq!(service.resume_calls.load(Ordering::SeqCst), 2);

        let commit_payload = ConsensusPayload::new(
            network,
            42,
            0,
            3,
            ConsensusMessageType::Commit,
            recovered_signature.clone(),
        );
        let mut recovery = neo_consensus::messages::RecoveryMessage::new(42, 3, 0);
        recovery
            .commit_messages
            .push(neo_consensus::messages::CommitPayloadCompact {
                view_number: 3,
                validator_index: 0,
                signature: recovered_signature.clone(),
                invocation_script: Vec::new(),
            });
        let recovery_payload = ConsensusPayload::new(
            network,
            42,
            0,
            3,
            ConsensusMessageType::RecoveryMessage,
            recovery.serialize(),
        );
        let block_data = BlockData {
            block_index: 42,
            timestamp: 0,
            nonce: 0,
            primary_index: 0,
            transaction_hashes: Vec::new(),
            signatures: vec![(0, recovered_signature)],
            validator_pubkeys: vec![neo_crypto::ECPoint::infinity(
                neo_crypto::ECCurve::Secp256r1,
            )],
            required_signatures: 1,
            commit_persistence: None,
        };
        let mut persistence_attempts = 0;

        let commit_broadcast = authorize_consensus_payload_egress(&commit_payload, Some(0), |_| {
            persistence_attempts += 1;
            true
        });
        let recovery_broadcast =
            authorize_consensus_payload_egress(&recovery_payload, Some(0), |_| {
                persistence_attempts += 1;
                true
            });
        let block_persistence = authorize_block_commit_egress(&block_data, Some(0), |_| {
            persistence_attempts += 1;
            true
        });

        assert!(!commit_broadcast, "Commit broadcast requires its permit");
        assert!(
            !recovery_broadcast,
            "RecoveryMessage carrying the recovered Commit requires its permit"
        );
        assert!(
            !block_persistence,
            "final block persistence requires the local Commit permit"
        );
        assert_eq!(
            persistence_attempts, 0,
            "missing permits must fail before durable persistence"
        );
    }

    #[test]
    fn commit_egress_fails_closed_when_persistence_fails() {
        let permit = valid_commit_permit();
        let mut persist_called = false;

        let authorized = authorize_commit_egress(
            Some(&permit),
            permit.block_index,
            permit.view_number,
            permit.validator_index,
            &permit.signature,
            |_| {
                persist_called = true;
                false
            },
        );

        assert!(
            persist_called,
            "valid permit must reach durable persistence"
        );
        assert!(
            !authorized,
            "signature egress must be withheld after persistence failure"
        );
    }
}
