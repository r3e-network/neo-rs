//! dBFT consensus wiring for neo-node.

use crate::config::DbftSettings;
use neo_consensus::{
    BlockData, ChangeViewReason, ConsensusContext, ConsensusEvent, ConsensusMessageType,
    ConsensusPayload, ConsensusService, ConsensusSigner, ValidatorInfo,
};
use neo_core::akka::{Actor, ActorContext, ActorRef, ActorResult, Cancelable, Props};
use neo_core::cryptography::MerkleTree;
use neo_core::i_event_handlers::IMessageReceivedHandler;
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
    register_message_received_handler, LocalNodeCommand, Message, MessageCommand,
    MessageHandlerSubscription, TaskManagerCommand,
};
use neo_core::persistence::IStore;
use neo_core::prelude::Serializable;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::native::ledger_contract::HashOrIndex;
use neo_core::smart_contract::native::{helpers::NativeHelpers, LedgerContract, NeoToken};
use neo_core::smart_contract::ContractParametersContext;
use neo_core::time_provider::TimeProvider;
use neo_core::wallets::Wallet;
use neo_core::{ContainsTransactionType, UInt160, UInt256};
use neo_vm::op_code::OpCode;
use neo_vm::ScriptBuilder;
use parking_lot::Mutex;
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

const CONSENSUS_STATE_KEY: [u8; 1] = [0xf4];

/// Wallet-changed handler that starts/stops the consensus actor.
pub struct DbftConsensusController {
    system: Arc<neo_core::neo_system::NeoSystem>,
    settings: DbftSettings,
    actor: Mutex<Option<ActorRef>>,
    _message_subscription: MessageHandlerSubscription,
}

impl DbftConsensusController {
    pub fn new(system: Arc<neo_core::neo_system::NeoSystem>, settings: DbftSettings) -> Self {
        let filter = Arc::new(DbftMessageFilter::new(settings.max_block_system_fee));
        let subscription = register_message_received_handler(filter);
        install_mempool_filter(&system, settings.max_block_system_fee);
        Self {
            system,
            settings,
            actor: Mutex::new(None),
            _message_subscription: subscription,
        }
    }

    pub fn start_with_wallet(&self, wallet: Arc<dyn Wallet>) -> bool {
        let mut guard = self.actor.lock();
        if guard.is_some() {
            return false;
        }

        let props =
            ConsensusActor::props(self.system.clone(), self.settings.clone(), wallet.clone());
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

impl neo_core::i_event_handlers::IWalletChangedHandler for DbftConsensusController {
    fn i_wallet_provider_wallet_changed_handler(
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

impl IMessageReceivedHandler for DbftMessageFilter {
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
}

impl WalletConsensusSigner {
    fn new(wallet: Arc<dyn Wallet>) -> Self {
        Self { wallet }
    }
}

impl ConsensusSigner for WalletConsensusSigner {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.wallet
            .get_account(script_hash)
            .is_some_and(|account| !account.is_locked() && account.has_key())
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> neo_consensus::ConsensusResult<Vec<u8>> {
        futures::executor::block_on(self.wallet.sign(data, script_hash)).map_err(|err| {
            neo_consensus::ConsensusError::state_error(format!("Wallet signing failed: {err}"))
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
    service: Option<ConsensusService>,
    event_task: Option<JoinHandle<()>>,
    timer: Option<Cancelable>,
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
    recovery_store: Option<Arc<dyn IStore>>,
    recovery_requested: bool,
}

impl ConsensusActor {
    fn new(
        system: Arc<neo_core::neo_system::NeoSystem>,
        settings: DbftSettings,
        wallet: Arc<dyn Wallet>,
    ) -> Self {
        let recovery_path = resolve_recovery_path(&settings.recovery_logs);
        let recovery_store = open_recovery_store(&system, &settings);
        Self {
            system,
            settings,
            wallet,
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
    ) -> Props {
        Props::new(move || Self::new(system.clone(), settings.clone(), wallet.clone()))
    }

    fn current_time_ms() -> u64 {
        let millis = TimeProvider::current().utc_now_timestamp_millis();
        if millis < 0 {
            0
        } else {
            millis as u64
        }
    }

    fn start_timer(&mut self, ctx: &ActorContext) {
        let block_ms = self.system.time_per_block().as_millis() as u64;
        let interval_ms = block_ms.saturating_div(5).max(200);
        let interval = Duration::from_millis(interval_ms);
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

    fn find_signing_key(
        &self,
        validators: &[neo_core::cryptography::ECPoint],
    ) -> (Option<u8>, Option<Vec<u8>>) {
        for (idx, pubkey) in validators.iter().enumerate() {
            let contract = Contract::create_signature_contract(pubkey.clone());
            let script_hash = contract.script_hash();
            let Some(account) = self.wallet.get_account(&script_hash) else {
                continue;
            };
            if account.is_locked() || !account.has_key() {
                continue;
            }
            let private_key = account
                .get_key()
                .map(|key_pair| key_pair.private_key().to_vec());
            return (Some(idx as u8), private_key);
        }
        (None, None)
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
        let (my_index, private_key) = self.find_signing_key(&validators);
        let signer = Arc::new(WalletConsensusSigner::new(self.wallet.clone()));

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
        let private_key = private_key.unwrap_or_default();

        let network = self.system.settings().network;
        let expected_block_time = self.system.time_per_block().as_millis() as u64;
        let now = Self::current_time_ms();

        let mut use_recovery = None;
        if !self.settings.ignore_recovery_logs {
            if let Some(context) =
                self.load_recovery_from_store(validator_infos.clone(), Some(my_index))
            {
                if context.block_index == block_index {
                    use_recovery = Some(context);
                } else {
                    warn!(target: "neo", "recovery log does not match current height; ignoring");
                }
            } else if self.recovery_path.exists() {
                match ConsensusContext::load(
                    &self.recovery_path,
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

        if let Some(context) = use_recovery {
            let (event_tx, event_rx) = mpsc::channel(256);
            let mut service =
                ConsensusService::with_context(network, context, private_key.clone(), event_tx);
            service.set_signer(Some(signer.clone()));
            service.set_expected_block_time(expected_block_time);
            if let Err(err) = service.resume(now, prev_hash, 0) {
                warn!(target: "neo", %err, "failed to resume consensus from recovery log");
            }
            self.install_service(service, event_rx, ctx);
            return;
        }

        if let Some(service) = self.service.as_mut() {
            service.update_validators(validator_infos, Some(my_index));
            service.set_private_key(private_key.clone());
            service.set_signer(Some(signer.clone()));
            service.set_expected_block_time(expected_block_time);
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
            private_key,
            event_tx,
        );
        service.set_signer(Some(signer));
        service.set_expected_block_time(expected_block_time);
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
        self.start_round(next_index, hash, block.timestamp(), ctx);
    }

    fn on_timer_tick(&mut self) {
        let now = Self::current_time_ms();
        self.try_prepare_response(now);
        self.try_finalize_pending_block(now);

        let Some(service) = self.service.as_mut() else {
            return;
        };
        if service.context().is_timed_out(now) {
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

            if let Err(err) = service.request_change_view(reason, now) {
                warn!(target: "neo", %err, "failed to request view change");
            }
        }
    }

    fn on_service_event(&mut self, event: ConsensusEvent) {
        match event {
            ConsensusEvent::BroadcastMessage(payload) => {
                self.broadcast_consensus_message(payload);
            }
            ConsensusEvent::RequestTransactions { max_count, .. } => {
                self.propose_transactions(max_count);
            }
            ConsensusEvent::BlockCommitted { block_data, .. } => {
                self.pending_block = Some(block_data);
                self.try_finalize_pending_block(Self::current_time_ms());
            }
            ConsensusEvent::ViewChanged { .. } => {
                self.reset_round_state();
            }
        }
    }

    fn broadcast_consensus_message(&mut self, payload: ConsensusPayload) {
        let Some(service) = self.service.as_ref() else {
            return;
        };

        if payload.message_type == ConsensusMessageType::Commit
            && !self.settings.ignore_recovery_logs
        {
            let mut saved = self.save_recovery_to_store(service);
            if !saved {
                if let Some(parent) = self.recovery_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(err) = service.save_context(&self.recovery_path) {
                    warn!(target: "neo", %err, "failed to persist consensus recovery log");
                } else {
                    saved = true;
                }
            }
            if !saved {
                warn!(target: "neo", "failed to persist consensus recovery log");
            }
        }

        let Some(extensible) = self.build_extensible_payload(&payload) else {
            return;
        };

        if let Err(err) = self
            .system
            .local_node_actor()
            .tell(LocalNodeCommand::SendDirectly {
                inventory: RelayInventory::Extensible(extensible),
                block_index: None,
            })
        {
            warn!(target: "neo", %err, "failed to broadcast consensus payload");
        }
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
        let Some(service) = self.service.as_mut() else {
            return;
        };

        let settings = self.system.settings();
        let max_tx = settings.max_transactions_per_block as usize;
        let limit = std::cmp::min(max_count, max_tx);

        let pool = self.system.mempool();
        let mut selected = Vec::new();
        let mut total_size = 0usize;
        let mut total_system_fee = 0i64;

        let required_signatures = consensus_m_threshold(self.validators.len());
        let header_size = estimate_header_size(&self.validators, required_signatures);

        for tx in pool.lock().get_sorted_verified_transactions(limit) {
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

        self.proposal_transactions = selected.iter().map(|tx| (tx.hash(), tx.clone())).collect();

        let hashes = selected.into_iter().map(|tx| tx.hash()).collect();
        if let Err(err) = service.on_transactions_received(hashes) {
            warn!(target: "neo", %err, "failed to submit proposal transactions");
        }
    }

    fn try_prepare_response(&mut self, now: u64) {
        let Some(service) = self.service.as_mut() else {
            return;
        };

        if service.context().is_primary() {
            return;
        }

        let my_index = match self.my_index {
            Some(index) => index,
            None => return,
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
            .map(|tx| (tx.hash(), tx))
            .collect();
        let unverified_map: HashMap<UInt256, Transaction> = pool_guard
            .unverified_transactions_vec()
            .into_iter()
            .map(|tx| (tx.hash(), tx))
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
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }

                if tx.system_fee() > self.settings.max_block_system_fee {
                    warn!(
                        target: "neo",
                        tx_hash = %hash,
                        "transaction system fee exceeds max_block_system_fee"
                    );
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }

                if conflicts_with_context(tx, &transactions) {
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
                }

                let result = tx.verify(settings, snapshot, Some(&verification_context), &[]);
                if result != VerifyResult::Succeed {
                    let reason = if result == VerifyResult::PolicyFail {
                        ChangeViewReason::TxRejectedByPolicy
                    } else {
                        ChangeViewReason::TxInvalid
                    };
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
                    .task_manager_actor()
                    .tell(TaskManagerCommand::RestartTasks { payload: inv })
                {
                    debug!(target: "neo", %err, "failed to request missing transactions");
                }
                self.missing_transactions = Some(missing);
            }
            return;
        }

        let total_system_fee = transactions.values().map(|tx| tx.system_fee()).sum::<i64>();
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
                .task_manager_actor()
                .tell(TaskManagerCommand::RestartTasks { payload: inv })
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

        let Some(witnesses) = context.get_witnesses() else {
            warn!(target: "neo", "failed to build block witness from commits");
            return;
        };
        header.witness = witnesses.into_iter().next().unwrap_or_else(Witness::new);

        let mut block = Block {
            header,
            transactions,
        };
        block.rebuild_merkle_root();

        if let Err(err) = self
            .system
            .blockchain_actor()
            .tell(neo_core::ledger::BlockchainCommand::InventoryBlock { block, relay: true })
        {
            warn!(target: "neo", %err, "failed to submit consensus block");
            return;
        }

        self.pending_block = None;
    }

    fn on_relay_result(&mut self, result: RelayResult) {
        if result.result != VerifyResult::Succeed {
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

        if let Some(prev_hash) = self.current_prev_hash {
            if message.prev_hash != prev_hash {
                return false;
            }
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
        let snapshot = store.get_snapshot();
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

    fn save_recovery_to_store(&self, service: &ConsensusService) -> bool {
        let store = match self.recovery_store.as_ref() {
            Some(store) => store,
            None => return false,
        };

        let Ok(data) = service.context().to_bytes() else {
            return false;
        };

        let mut snapshot = store.get_snapshot();
        let Some(snap) = Arc::get_mut(&mut snapshot) else {
            return false;
        };
        snap.put_sync(CONSENSUS_STATE_KEY.to_vec(), data);
        if let Err(err) = snap.try_commit() {
            warn!(target: "neo", %err, "failed to commit consensus recovery log");
            return false;
        }

        true
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
                self.on_persist_completed(message.block, _ctx);
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
                ConsensusActorMessage::ServiceEvent(event) => self.on_service_event(event),
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
    let candidate = PathBuf::from(value);
    if candidate.is_absolute() {
        candidate
    } else {
        candidate
    }
}

fn open_recovery_store(
    system: &neo_core::neo_system::NeoSystem,
    settings: &DbftSettings,
) -> Option<Arc<dyn IStore>> {
    if settings.ignore_recovery_logs {
        return None;
    }
    let path = settings.recovery_logs.trim();
    if path.is_empty() {
        return None;
    }
    match system.store_provider().get_store(path) {
        Ok(store) => Some(store),
        Err(err) => {
            warn!(target: "neo", %err, "failed to open consensus recovery store");
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
    if invocation[0] != OpCode::PUSHDATA1 as u8 || invocation[1] != 0x40 {
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
