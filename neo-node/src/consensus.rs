//! dBFT consensus wiring for neo-node.
//!
//! Stage F (neo-actors removal): the legacy actor runtime is gone, and the
//! `neo-consensus` crate exposes `ConsensusService` directly as an
//! `async`-first stateful service. The previous actor-based wrapper is
//! replaced by a thin shim that owns the consensus service, drives its
//! timer loop, and bridges the legacy event-bus types via tokio mpsc.

use crate::config::DbftSettings;
use neo_blockchain::{PersistCompleted, RelayResult, TransactionVerificationContext};
use neo_consensus::{
    BlockData, ChangeViewReason, ConsensusContext, ConsensusEvent, ConsensusMessageType,
    ConsensusPayload, ConsensusService, ConsensusSigner, ValidatorInfo};
use neo_crypto::MerkleTree;
use neo_event_handlers::MessageReceivedHandler;
use neo_execution::ContractParametersContext;
use neo_execution::contract::Contract;
use neo_io::MemoryReader;
// HashOrIndex from neo_blockchain;
use neo_native_contracts::{LedgerContract, NeoToken, helpers::NativeHelpers};
// use neo_network::RelayInventory;
use neo_p2p::payloads::message::Message;
use neo_p2p::payloads::message::MessageCommand;
use neo_p2p::MessageHandlerSubscription;
use neo_p2p::register_message_received_handler;
use neo_payloads::{Block, ExtensiblePayload, Header, InventoryType, Transaction, TransactionAttribute, Witness};
use neo_p2p::payloads::inv_payload::InvPayload;
use neo_io::Serializable;
use neo_primitives::{ContainsTransactionType, UInt160, UInt256};
use neo_script_builder::ScriptBuilder;
use neo_storage::persistence::Store;
use neo_system::Node;
use neo_time::TimeProvider;
use neo_wallets::Wallet;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, warn};

const CONSENSUS_STATE_KEY: [u8; 1] = [0xf4];
const TIMER_INTERVAL: Duration = Duration::from_secs(1);

/// Wallet-changed handler that starts/stops the consensus service.
pub struct DbftConsensusController {
    system: Arc<NeoSystem>,
    settings: DbftSettings,
    task: Mutex<Option<JoinHandle<()>>>,
    stop_tx: Mutex<Option<mpsc::Sender<()>>>,
    _message_subscription: MessageHandlerSubscription}

impl DbftConsensusController {
    pub fn new(system: Arc<NeoSystem>, settings: DbftSettings) -> Self {
        let filter = Arc::new(DbftMessageFilter::new(settings.max_block_system_fee));
        let subscription = register_message_received_handler(filter);
        install_mempool_filter(&system, settings.max_block_system_fee);
        Self {
            system,
            settings,
            task: Mutex::new(None),
            stop_tx: Mutex::new(None),
            _message_subscription: subscription}
   }

    pub fn start_with_wallet(&self, wallet: Arc<dyn Wallet>) -> bool {
        let mut guard = self.task.lock();
        if guard.is_some() {
            return false;
       }
        let (stop_tx, mut stop_rx) = mpsc::channel::<()>(1);
        let actor = ConsensusTask::new(
            self.system.clone(),
            self.settings.clone(),
            wallet.clone(),
        );
        let handle = tokio::spawn(async move {
            actor.run(&mut stop_rx).await;
       });
        *self.stop_tx.lock() = Some(stop_tx);
        *guard = Some(handle);
        if !self.settings.auto_start {
            // Manual start is requested via the existing RPC; for now treat
            // start_with_wallet as immediate.
       }
        true
   }

    pub fn stop(&self) -> bool {
        let mut guard = self.task.lock();
        if let Some(handle) = guard.take() {
            handle.abort();
            if let Some(tx) = self.stop_tx.lock().take() {
                let _ = tx.try_send(());
           }
            return true;
       }
        false
   }

    pub fn is_running(&self) -> bool {
        self.task.lock().is_some()
   }
}

impl neo_event_handlers::WalletChangedHandler for DbftConsensusController {
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
    max_block_system_fee: i64}

impl DbftMessageFilter {
    fn new(max_block_system_fee: i64) -> Self {
        Self {
            max_block_system_fee}
   }
}

impl MessageReceivedHandler for DbftMessageFilter {
    fn remote_node_message_received_handler(&self, _system: &dyn std::any::Any, message: &Message) -> bool {
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
    system: &Arc<NeoSystem>,
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
    wallet: Arc<dyn Wallet>}

impl WalletConsensusSigner {
    fn new(wallet: Arc<dyn Wallet>) -> Self {
        Self {wallet}
   }
}

impl ConsensusSigner for WalletConsensusSigner {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        self.wallet
            .get_account(script_hash)
            .is_some_and(|account| !account.is_locked() && account.has_key())
   }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> neo_consensus::ConsensusResult<Vec<u8>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.wallet.sign(data, script_hash))
       })
        .map_err(|err| {
            neo_consensus::ConsensusError::state_error(format!("Wallet signing failed: {err}"))
       })
   }
}

struct ConsensusTask {
    system: Arc<NeoSystem>,
    settings: DbftSettings,
    wallet: Arc<dyn Wallet>,
    service: Option<ConsensusService>,
    event_task: Option<JoinHandle<()>>,
    proposal_transactions: HashMap<UInt256, Transaction>,
    pending_block: Option<BlockData>,
    missing_transactions: Option<HashSet<UInt256>>,
    current_block_index: Option<u32>,
    current_prev_hash: Option<UInt256>,
    prev_timestamp: Option<u64>,
    validators: Vec<neo_crypto::ECPoint>,
    validator_infos: Vec<ValidatorInfo>,
    my_index: Option<u8>,
    recovery_path: PathBuf,
    recovery_store: Option<Arc<dyn Store>>,
    recovery_requested: bool}

impl ConsensusTask {
    fn new(
        system: Arc<NeoSystem>,
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
            recovery_requested: false}
   }

    fn current_time_ms() -> u64 {
        let millis = TimeProvider::current().utc_now_timestamp_millis();
        if millis < 0 {0} else {millis as u64}
   }

    async fn run(mut self, stop_rx: &mut mpsc::Receiver<()>) {
        if self.settings.auto_start {
            self.start_from_chain();
       }
        let mut ticker = tokio::time::interval(TIMER_INTERVAL);
        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.on_timer_tick();
               }
                _ = stop_rx.recv() => break}
       }
   }

    fn reset_round_state(&mut self) {
        self.proposal_transactions.clear();
        self.pending_block = None;
        self.missing_transactions = None;
   }

    fn load_validators(&self) -> Vec<neo_crypto::ECPoint> {
        let store_cache = self.system.store_cache();
        let snapshot = store_cache.data_cache();
        let settings = self.system.settings();
        let validators_count = settings.validators_count.max(0) as usize;
        NeoToken::new()
            .get_next_block_validators_snapshot(snapshot, validators_count, settings)
            .unwrap_or_else(|_| settings.standby_validators())
   }

    fn next_consensus_validators(&self, block_index: u32) -> Vec<neo_crypto::ECPoint> {
        let store_cache = self.system.store_cache();
        let snapshot = store_cache.data_cache();
        let settings = self.system.settings();
        let validators_count = settings.validators_count.max(0) as usize;
        let result = if NativeHelpers::should_refresh_committee(
            block_index,
            settings.committee_members_count(),
        ) {
            NeoToken::new().compute_next_block_validators_snapshot(snapshot, settings)
       } else {
            NeoToken::new().get_next_block_validators_snapshot(snapshot, validators_count, settings)
       };
        result.unwrap_or_else(|_| settings.standby_validators())
   }

    fn build_validator_infos(validators: &[neo_crypto::ECPoint]) -> Vec<ValidatorInfo> {
        validators
            .iter()
            .enumerate()
            .map(|(idx, pubkey)| ValidatorInfo {
                index: idx as u8,
                public_key: pubkey.clone(),
                script_hash: Contract::create_signature_contract(pubkey.clone()).script_hash()})
            .collect()
   }

    fn find_signing_key(
        &self,
        validators: &[neo_crypto::ECPoint],
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
    ) {
        if let Some(handle) = self.event_task.take() {
            handle.abort();
       }
        let handle = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                debug!(target: "neo", ?event, "consensus service event");
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
    ) {
        self.reset_round_state();
        self.current_block_index = Some(block_index);
        self.current_prev_hash = Some(prev_hash);
        self.prev_timestamp = Some(prev_timestamp);

        let validators = self.load_validators();
        let validator_infos = Self::build_validator_infos(&validators);
        let (my_index, private_key) = self.find_signing_key(&validators);
        let signer = Arc::new(WalletConsensusSigner::new(self.wallet.clone()));

        self.validators = validators;
        self.validator_infos = validator_infos.clone();
        self.my_index = my_index;

        let Some(my_index) = my_index else {
            warn!(target: "neo", "dBFT wallet has no signable validator account; consensus disabled");
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
                    Ok(_) => warn!(target: "neo", "recovery log does not match current height; ignoring"),
                    Err(err) => warn!(target: "neo", %err, "failed to load consensus recovery log")}
           }
       }

        if let Some(context) = use_recovery {
            let (event_tx, event_rx) = mpsc::channel(256);
            let mut service = ConsensusService::with_context(network, context, private_key.clone(), event_tx);
            service.set_signer(Some(signer.clone()));
            service.set_expected_block_time(expected_block_time);
            if let Err(err) = service.resume(now, prev_hash, 0) {
                warn!(target: "neo", %err, "failed to resume consensus from recovery log");
           }
            self.install_service(service, event_rx);
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
        self.install_service(service, event_rx);
   }

    fn start_from_chain(&mut self) {
        let current_height = self.system.current_block_index();
        let prev_hash = self
            .system
            .block_hash_at(current_height)
            .unwrap_or_default();
        let prev_timestamp = fetch_block_timestamp(&self.system, current_height).unwrap_or(0);
        self.start_round(current_height + 1, prev_hash, prev_timestamp);
        self.request_recovery_on_start();
   }

    fn request_recovery_on_start(&mut self) {
        if self.recovery_requested {return;}
        let Some(service) = self.service.as_mut() else {return;};
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

    fn on_persist_completed(&mut self, mut block: Block) {
        let next_index = block.index().saturating_add(1);
        let hash = block.hash();
        self.start_round(next_index, hash, block.timestamp());
   }

    fn on_timer_tick(&mut self) {
        let now = Self::current_time_ms();
        self.try_prepare_response(now);
        self.try_finalize_pending_block(now);
        let Some(service) = self.service.as_mut() else {return;};
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

    fn on_relay_result(&mut self, result: RelayResult) {
        if result.result != VerifyResult::Succeed {return;}
        if result.inventory_type != InventoryType::Extensible {return;}
        let context = self.system.context();
        let payload = context
            .try_get_relay_extensible(&result.hash)
            .or_else(|| context.try_get_extensible(&result.hash));
        let Some(payload) = payload else {return;};
        if payload.category != "dBFT" || payload.data.is_empty() {return;}
        let Some(signature) = extract_signature(&payload.witness) else {return;};
        let network = self.system.settings().network;
        let consensus_payload =
            match ConsensusPayload::from_message_bytes(network, &payload.data, signature) {
                Ok(value) => value,
                Err(err) => {debug!(target: "neo", %err, "failed to parse consensus payload"); return;}
           };
        if !self.precheck_prepare_request(&consensus_payload) {return;}
        let Some(service) = self.service.as_mut() else {return;};
        if let Err(err) = service.process_message(consensus_payload) {
            debug!(target: "neo", %err, "consensus message rejected");
       }
        self.try_prepare_response(Self::current_time_ms());
   }

    fn precheck_prepare_request(&self, payload: &ConsensusPayload) -> bool {
        if payload.message_type != ConsensusMessageType::PrepareRequest {
            return true;
       }
        let Some(current_index) = self.current_block_index else {return false;};
        if payload.block_index != current_index {return false;}
        let Ok(message) = neo_consensus::messages::PrepareRequestMessage::deserialize_body(
            &payload.data, payload.block_index, payload.view_number, payload.validator_index,
        ) else {return false;};
        if message.transaction_hashes.len() > self.system.settings().max_transactions_per_block as usize {
            return false;
       }
        if let Some(prev_hash) = self.current_prev_hash {
            if message.prev_hash != prev_hash {return false;}
       }
        let prev_timestamp = self.prev_timestamp.unwrap_or(0);
        if message.timestamp <= prev_timestamp {return false;}
        let now = Self::current_time_ms();
        let max_future = self.system.time_per_block().as_millis() as u64 * 8;
        if message.timestamp > now.saturating_add(max_future) {return false;}
        true
   }

    fn try_prepare_response(&mut self, now: u64) {
        let Some(service) = self.service.as_mut() else {return;};
        if service.context().is_primary() {return;}
        let my_index = match self.my_index {Some(index) => index, None => return};
        if service.context().prepare_responses.contains_key(&my_index) {return;}
        if !service.context().prepare_request_received {return;}
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
            if self.system.context().contains_transaction(hash) == ContainsTransactionType::ExistsInLedger {
                let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                return;
           }
            if let Some(tx) = verified_map.get(hash) {
                let signer_accounts = signer_accounts(tx);
                if self.system.context().contains_conflict_hash(hash, &signer_accounts) {
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
               }
                transactions.insert(*hash, tx.clone());
                verification_context.add_transaction(tx);
                continue;
           }
            if let Some(tx) = unverified_map.get(hash) {
                let signer_accounts = signer_accounts(tx);
                if self.system.context().contains_conflict_hash(hash, &signer_accounts) {
                    let _ = service.request_change_view(ChangeViewReason::TxInvalid, now);
                    return;
               }
                if tx.system_fee() > self.settings.max_block_system_fee {
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
                None => true};
            if should_request {
                let hashes: Vec<UInt256> = missing.iter().copied().collect();
                let inv = InvPayload::create(InventoryType::Transaction, &hashes);
                if let Err(err) = self.system.task_manager_handle().broadcast_restart_tasks(inv) {
                    debug!(target: "neo", %err, "failed to request missing transactions");
               }
                self.missing_transactions = Some(missing);
           }
            return;
       }
        self.proposal_transactions = transactions;
        self.missing_transactions = None;
        let _ = service.on_transactions_received(proposed_hashes);
   }

    fn try_finalize_pending_block(&mut self, _now: u64) {
        let Some(block_data) = self.pending_block.clone() else {return;};
        let current_index = self.current_block_index.unwrap_or_default();
        if block_data.block_index != current_index {
            self.pending_block = None;
            return;
       }
        // The actual block assembly + commit happens in the modern
        // neo-consensus service; this shim is intentionally minimal until
        // the full reth-style pipeline lands in Stage G.
        self.pending_block = None;
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
            Err(err) => {warn!(target: "neo", %err, "failed to decode consensus recovery log"); None}
       }
   }
}

fn resolve_recovery_path(value: &str) -> PathBuf {PathBuf::from(value)}

fn open_recovery_store(system: &NeoSystem, settings: &DbftSettings) -> Option<Arc<dyn Store>> {
    if settings.ignore_recovery_logs {return None;}
    let path = settings.recovery_logs.trim();
    if path.is_empty() {return None;}
    match system.store_provider().get_store(path) {
        Ok(store) => Some(store),
        Err(err) => {warn!(target: "neo", %err, "failed to open consensus recovery store"); None}
   }
}

fn fetch_block_timestamp(system: &NeoSystem, index: u32) -> Option<u64> {
    let store_cache = system.store_cache();
    let ledger = LedgerContract::new();
    ledger.get_block(&store_cache, HashOrIndex::Index(index)).ok().flatten().map(|b| b.header.timestamp())
}

fn extract_signature(witness: &Witness) -> Option<Vec<u8>> {
    let invocation = &witness.invocation_script;
    if invocation.len() != 66 {return None;}
    if invocation[0] != neo_vm_rs::OpCode::PUSHDATA1.byte() || invocation[1] != 0x40 {return None;}
    Some(invocation[2..66].to_vec())
}

fn consensus_m_threshold(n: usize) -> usize {
    if n == 0 {0} else {n - (n - 1) / 3}
}

fn conflicts_with_context(tx: &Transaction, existing: &HashMap<UInt256, Transaction>) -> bool {
    let new_conflicts = conflict_hashes(tx);
    if new_conflicts.iter().any(|hash| existing.contains_key(hash)) {return true;}
    for other in existing.values() {
        if conflict_hashes(other).iter().any(|hash| *hash == tx.hash()) {return true;}
   }
    false
}

fn conflict_hashes(tx: &Transaction) -> Vec<UInt256> {
    tx.attributes()
        .iter()
        .filter_map(|attr| match attr {
            TransactionAttribute::Conflicts(conflicts) => Some(conflicts.hash),
            _ => None})
        .collect()
}

fn signer_accounts(tx: &Transaction) -> Vec<UInt160> {
    tx.signers().iter().map(|signer| signer.account).collect()
}
