use super::*;

use super::controller::WalletConsensusSigner;
use super::policy::fetch_block_timestamp;
use super::recovery::{accept_recovery_context, open_recovery_store, resolve_recovery_path};

use neo_actors::{Actor, ActorContext, ActorResult, Props};
use neo_consensus::{
    ChangeViewReason, ConsensusContext, ConsensusEvent, ConsensusPayload, RecoveryResumeAction,
    recovery_resume_action, should_request_recovery_on_start,
};
use neo_ledger::PersistCompleted;
use neo_p2p::p2p::local_node::RelayInventory;
use neo_primitives::time_provider::TimeProvider;
use neo_protocol::payloads::Block;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub(super) enum ConsensusActorMessage {
    ServiceEvent(ConsensusEvent),
    TimerTick,
    ManualStart,
}
impl ConsensusActor {
    fn new(
        system: Arc<neo_system::NeoSystem>,
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

    pub(super) fn props(
        system: Arc<neo_system::NeoSystem>,
        settings: DbftSettings,
        wallet: Arc<dyn Wallet>,
    ) -> Props {
        Props::new(move || Self::new(system.clone(), settings.clone(), wallet.clone()))
    }

    pub(super) fn current_time_ms() -> u64 {
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
        debug!(
            target: "neo::consensus",
            event,
            view = context.view_number,
            phase,
            role = if context.is_primary() { "primary" } else { "backup" },
            prepare_votes = context.prepare_responses.len(),
            commit_votes = context.commits.len(),
            missing_transactions = self.missing_transactions.as_ref().map_or(0, HashSet::len),
            recovery_active = service.is_recovering(),
            signing_authority_available = self.my_index.is_some(),
            active_validators = self.validators.len(),
            expected_validators = self.validators.len(),
            "consensus state snapshot"
        );
    }

    fn load_validators(&self, next_index: u32) -> Vec<neo_crypto::ECPoint> {
        load_validators(&self.system, next_index)
    }

    fn build_validator_infos(validators: &[neo_crypto::ECPoint]) -> Vec<ValidatorInfo> {
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

    fn find_signing_key(&self, validators: &[neo_crypto::ECPoint]) -> Option<u8> {
        find_signing_key(&self.wallet, validators)
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
        let signer = Arc::new(WalletConsensusSigner::new(self.wallet.clone()));

        self.validators = validators;
        self.validator_infos.clone_from(&validator_infos);
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
                use_recovery = accept_recovery_context(
                    self.load_recovery_from_store(validator_infos.clone(), Some(my_index)),
                    block_index,
                );
            } else {
                let fallback_path = if self.recovery_path.is_dir() {
                    self.recovery_path.join("consensus.context")
                } else {
                    self.recovery_path.clone()
                };
                if fallback_path.is_file() {
                    let loaded = match ConsensusContext::load(
                        &fallback_path,
                        validator_infos.clone(),
                        Some(my_index),
                    ) {
                        Ok(context) => Some(context),
                        Err(err) => {
                            warn!(target: "neo", %err, "failed to load consensus recovery log");
                            None
                        }
                    };
                    use_recovery = accept_recovery_context(loaded, block_index);
                }
            }
        }

        if let Some(context) = use_recovery {
            let resume_action = recovery_resume_action(&context);
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
            match resume_action {
                RecoveryResumeAction::AnnounceEmptyProposal => {
                    if let Some(service) = self.service.as_mut() {
                        let _ = service.on_transactions_received(Vec::new());
                    }
                }
                RecoveryResumeAction::RebuildPrepareResponse => self.try_prepare_response(now),
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

        let outcome = if should_request_recovery_on_start(service.context()) {
            Some(service.request_recovery())
        } else {
            None
        };
        self.recovery_requested = true;
        if let Some(Err(err)) = outcome {
            warn!(target: "neo", %err, "failed to request consensus recovery");
        }
    }

    fn on_persist_completed(&mut self, mut block: Block, ctx: &ActorContext) {
        let next_index = block.index().saturating_add(1);
        let hash = block.hash();
        debug!(
            target: "neo::consensus",
            block_index = block.index(),
            block_hash = %hash,
            transaction_count = block.transactions.len(),
            "consensus block persisted"
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
            .is_some_and(|missing| !missing.is_empty())
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
}

#[async_trait::async_trait]
impl Actor for ConsensusActor {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        let stream = self.system.event_stream();
        stream.subscribe::<PersistCompleted>(ctx.self_ref());
        stream.subscribe::<neo_p2p::p2p::ports::RelayOutcome>(ctx.self_ref());
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
                self.on_persist_completed(message.block.to_payload_block(), _ctx);
                return Ok(());
            }
            Err(envelope) => envelope,
        };

        let envelope = match envelope.downcast::<neo_p2p::p2p::ports::RelayOutcome>() {
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
