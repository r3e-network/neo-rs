// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_service.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_context::ConsensusContext;
use crate::dbft_plugin::dbft_settings::DbftSettings;
use crate::dbft_plugin::types::change_view_reason::ChangeViewReason;
use neo_core::network::p2p::local_node::LocalNodeCommand;
use neo_core::network::p2p::payloads::inv_payload::InvPayload;
use neo_core::network::p2p::payloads::inventory_type::InventoryType;
use neo_core::network::p2p::payloads::ExtensiblePayload;
use neo_core::network::p2p::task_manager::TaskManagerCommand;
use neo_core::network::p2p::RelayInventory;
use neo_core::persistence::IStore;
use neo_core::{NeoSystem, Transaction, UInt256};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::info;

/// Start message for consensus service
/// Matches C# Start class
#[derive(Debug, Clone)]
pub struct Start;

/// Timer message for consensus service
/// Matches C# Timer class
#[derive(Debug, Clone)]
pub struct ConsensusTimer {
    pub height: u32,
    pub view_number: u8,
}

/// Consensus Service implementation matching C# ConsensusService exactly
pub struct ConsensusService {
    /// Consensus context
    pub(crate) context: Arc<RwLock<ConsensusContext>>,
    /// Whether service is started
    started: bool,
    /// DBFT settings
    pub(crate) dbft_settings: DbftSettings,
    /// Neo system reference
    pub(crate) neo_system: Arc<NeoSystem>,
    /// Timestamp of the last timer change
    pub(crate) clock_started: Instant,
    /// Expected delay for the active timer
    pub(crate) expected_delay: Duration,
    /// Timestamp of the last prepare request received
    pub(crate) prepare_request_received_time: Option<Instant>,
    /// Block index accompanying the last prepare request
    pub(crate) prepare_request_received_block_index: u32,
    /// Highest block index observed in commit processing
    pub(crate) block_received_index: u32,
    /// Recently processed payload hashes (prevents duplicate responses)
    pub(crate) known_hashes: HashSet<UInt256>,
    /// Whether the service is currently replaying recovery messages
    pub(crate) is_recovering: bool,
}

impl ConsensusService {
    /// Creates a new ConsensusService
    /// Matches C# constructor with NeoSystem, DbftSettings, ISigner
    pub fn new(
        neo_system: Arc<NeoSystem>,
        settings: DbftSettings,
        signer: Arc<dyn neo_core::sign::ISigner>,
    ) -> Self {
        let context = Arc::new(RwLock::new(ConsensusContext::new(
            neo_system.clone(),
            settings.clone(),
            signer,
        )));

        Self {
            context,
            started: false,
            dbft_settings: settings,
            neo_system,
            clock_started: Instant::now(),
            expected_delay: Duration::from_millis(0),
            prepare_request_received_time: None,
            prepare_request_received_block_index: 0,
            block_received_index: 0,
            known_hashes: HashSet::new(),
            is_recovering: false,
        }
    }

    /// Injects a persistence store into the consensus context for state recovery/persistence.
    pub async fn set_store(&self, store: std::sync::Arc<dyn IStore>) {
        let mut context = self.context.write().await;
        context.set_store(store);
    }

    /// Starts the consensus service
    /// Matches C# OnStart method
    pub async fn start(&mut self) {
        if self.started {
            return;
        }

        self.log("OnStart");
        self.started = true;

        let (view_number, watch_only, should_check_preparations) = {
            let mut context = self.context.write().await;
            let mut check_preparations = false;

            if !self.dbft_settings.ignore_recovery_logs && context.load() && context.commit_sent() {
                check_preparations = true;
            }

            (
                context.view_number(),
                context.watch_only(),
                check_preparations,
            )
        };

        if should_check_preparations {
            self.check_preparations().await;
            return;
        }

        self.initialize_consensus(view_number).await;

        if !watch_only {
            self.request_recovery().await;
        }
    }

    /// Handles timer events
    /// Matches C# OnTimer method
    pub async fn on_timer(&mut self, timer: ConsensusTimer) {
        enum TimerAction {
            None,
            SendPrepareRequest,
            ResendCommit,
            RequestChangeView(ChangeViewReason),
        }

        let action = {
            let context = self.context.read().await;

            if context.watch_only()
                || context.block_sent()
                || timer.height != context.block().index()
                || timer.view_number != context.view_number()
            {
                TimerAction::None
            } else if context.is_primary() && !context.request_sent_or_received() {
                TimerAction::SendPrepareRequest
            } else if (context.is_primary() && context.request_sent_or_received())
                || context.is_backup()
            {
                if context.commit_sent() {
                    TimerAction::ResendCommit
                } else {
                    let missing_transactions =
                        match (context.transaction_hashes(), context.transactions()) {
                            (Some(hashes), Some(transactions)) => hashes.len() > transactions.len(),
                            (Some(hashes), None) => !hashes.is_empty(),
                            _ => false,
                        };

                    let reason = if missing_transactions {
                        ChangeViewReason::TxNotFound
                    } else {
                        ChangeViewReason::Timeout
                    };

                    TimerAction::RequestChangeView(reason)
                }
            } else {
                TimerAction::None
            }
        };

        match action {
            TimerAction::None => {}
            TimerAction::SendPrepareRequest => self.send_prepare_request().await,
            TimerAction::RequestChangeView(reason) => self.request_change_view(reason).await,
            TimerAction::ResendCommit => {
                let (payload, delay) = {
                    let mut context = self.context.write().await;
                    self.log("Sending RecoveryMessage to resend Commit");
                    let payload = context.make_recovery_message();
                    let delay = context.time_per_block.saturating_mul(2);
                    (payload, delay)
                };

                self.broadcast_payload(payload);
                self.change_timer(delay);
            }
        }
    }

    /// Handles transaction events
    /// Matches C# OnTransaction method
    pub async fn handle_transaction(&mut self, transaction: Transaction) {
        let should_accept = {
            let mut context = self.context.write().await;

            if !context.is_backup()
                || context.not_accepting_payloads_due_to_view_changing()
                || !context.request_sent_or_received()
                || context.response_sent()
                || context.block_sent()
            {
                return;
            }

            if let Some(transactions) = context.transactions() {
                if transactions.contains_key(&transaction.hash()) {
                    return;
                }
            }

            matches!(
                context.transaction_hashes(),
                Some(hashes) if hashes.contains(&transaction.hash())
            )
        };

        if should_accept {
            self.add_transaction(transaction, true).await;
        }
    }

    // Private helper methods

    pub(crate) async fn initialize_consensus(&mut self, view_number: u8) {
        let mut context = self.context.write().await;
        context.reset(view_number);

        if view_number > 0 {
            let previous_primary = context.get_primary_index(view_number.saturating_sub(1));
            if let Some(validator) = context.validators().get(previous_primary as usize) {
                self.log(&format!(
                    "View changed: view={} primary={:?}",
                    view_number, validator
                ));
            } else {
                self.log(&format!(
                    "View changed: view={} primary=<unknown>",
                    view_number
                ));
            }
        }

        let role = if context.is_primary() {
            "Primary"
        } else if context.watch_only() {
            "WatchOnly"
        } else {
            "Backup"
        };

        self.log(&format!(
            "Initialize: height={} view={} index={} role={}",
            context.block().index(),
            view_number,
            context.my_index,
            role,
        ));

        self.clock_started = Instant::now();
        self.expected_delay = context.time_per_block;
        self.prepare_request_received_time = None;
        self.prepare_request_received_block_index = 0;
        self.is_recovering = false;
        self.block_received_index = 0;
        self.known_hashes.clear();
    }

    async fn send_prepare_request(&mut self) {
        let (payload, should_check_preparations, transaction_hashes, delay) = {
            let mut context = self.context.write().await;
            self.log(&format!(
                "Sending PrepareRequest: height={} view={}",
                context.block().index(),
                context.view_number(),
            ));

            let payload = context.make_prepare_request();
            let should_check_preparations = context.validators().len() == 1;
            let transaction_hashes = context
                .transaction_hashes()
                .map(|hashes| hashes.to_vec())
                .unwrap_or_default();
            let delay = self.scaled_block_delay(
                context.time_per_block,
                (context.view_number() as u32).saturating_add(1),
            );

            (
                payload,
                should_check_preparations,
                transaction_hashes,
                delay,
            )
        };

        self.broadcast_payload(payload);

        if should_check_preparations {
            self.check_preparations().await;
        }

        if !transaction_hashes.is_empty() {
            self.request_missing_transactions(&transaction_hashes);
        }

        self.change_timer(delay);
    }

    async fn request_recovery(&mut self) {
        let payload = {
            let mut context = self.context.write().await;
            self.log(&format!(
                "Sending RecoveryRequest: height={} view={} nc={} nf={}",
                context.block().index(),
                context.view_number(),
                context.count_committed(),
                context.count_failed(),
            ));
            context.make_recovery_request()
        };

        self.broadcast_payload(payload);
    }

    pub(crate) async fn request_change_view(&mut self, reason: ChangeViewReason) {
        let (payload, need_recovery, expected_view, delay) = {
            let mut context = self.context.write().await;

            if context.watch_only() {
                return;
            }

            let expected_view = context.view_number().wrapping_add(1);
            self.log(&format!(
                "Sending ChangeView: height={} view={} nv={} nc={} nf={} reason={:?}",
                context.block().index(),
                context.view_number(),
                expected_view,
                context.count_committed(),
                context.count_failed(),
                reason,
            ));

            let delay = self.scaled_block_delay(
                context.time_per_block,
                (expected_view as u32).saturating_add(1),
            );

            let need_recovery = context.count_committed() + context.count_failed() > context.f();
            let payload = if need_recovery {
                None
            } else {
                Some(context.make_change_view(reason))
            };

            (payload, need_recovery, expected_view, delay)
        };

        self.change_timer(delay);

        if let Some(payload) = payload {
            self.broadcast_payload(payload);
            self.check_expected_view(expected_view).await;
        } else if need_recovery {
            self.request_recovery().await;
        }
    }

    async fn add_transaction(&mut self, tx: Transaction, verify: bool) -> bool {
        if verify {
            // Transaction verification will be wired once mempool integration is available.
        }

        {
            let mut context = self.context.write().await;
            if let Some(transactions) = context.transactions_mut() {
                transactions.insert(tx.hash(), tx.clone());
            }
        }

        self.check_prepare_response().await
    }

    pub(crate) fn broadcast_payload(&self, payload: ExtensiblePayload) {
        let mut payload_clone = payload.clone();
        let hash = payload_clone.hash();
        let block_index = self
            .context
            .try_read()
            .map(|guard| guard.block().index())
            .unwrap_or(0);

        let result = self
            .neo_system
            .local_node_actor()
            .tell(LocalNodeCommand::SendDirectly {
                inventory: RelayInventory::Extensible(payload),
                block_index: Some(block_index),
            });

        if let Err(err) = result {
            self.log(&format!(
                "Failed to broadcast payload category={} hash={hash}: {err}",
                payload_clone.category
            ));
        } else {
            self.log(&format!(
                "Broadcast payload category={} hash={hash}",
                payload_clone.category
            ));
        }
    }

    pub(crate) fn request_missing_transactions(&self, hashes: &[UInt256]) {
        if hashes.is_empty() {
            return;
        }
        self.log(&format!("Requesting {} missing transactions", hashes.len()));

        let payload = InvPayload::create(InventoryType::Transaction, hashes);
        let sender = self.neo_system.local_node_actor();
        let _ = self
            .neo_system
            .task_manager_actor()
            .tell_from(TaskManagerCommand::RestartTasks { payload }, Some(sender));
    }

    pub(crate) fn change_timer(&mut self, delay: Duration) {
        self.clock_started = Instant::now();
        self.expected_delay = delay;
    }

    pub(crate) async fn extend_timer_async(&mut self, factor: i32) {
        if factor <= 0 {
            return;
        }

        let info = {
            let mut context = self.context.write().await;
            if context.watch_only() || context.view_changing() || context.commit_sent() {
                None
            } else {
                Some((context.time_per_block, context.m()))
            }
        };

        let Some((time_per_block, validator_m)) = info else {
            return;
        };

        let elapsed = self.clock_started.elapsed();
        let mut remaining = self.expected_delay.saturating_sub(elapsed);

        if validator_m > 0 {
            let ratio = factor as f64 / validator_m as f64;
            if ratio.is_finite() && ratio > 0.0 {
                let additional = time_per_block.mul_f64(ratio);
                remaining = remaining.saturating_add(additional);
            }
        }

        if remaining > Duration::from_secs(0) {
            self.change_timer(remaining);
        }
    }

    pub(crate) fn extend_timer_by_factor(
        &mut self,
        state: &TimerContextState,
        max_delay_in_block_times: i32,
    ) {
        if max_delay_in_block_times <= 0 {
            return;
        }

        if state.should_skip() {
            return;
        }

        let elapsed = self.clock_started.elapsed();
        let mut remaining = self.expected_delay.saturating_sub(elapsed);

        if state.validator_threshold > 0 {
            let factor = max_delay_in_block_times as f64 / state.validator_threshold as f64;
            if factor.is_finite() && factor > 0.0 {
                let additional = state.time_per_block.mul_f64(factor);
                remaining = remaining.saturating_add(additional);
            }
        }

        if remaining > Duration::from_secs(0) {
            self.change_timer(remaining);
        }
    }

    fn scaled_block_delay(&self, base: Duration, shift: u32) -> Duration {
        if shift >= 31 {
            return Duration::MAX;
        }
        base.saturating_mul(1u32 << shift)
    }

    pub(crate) fn log(&self, message: &str) {
        info!(target: "dbft::consensus_service", "{}", message);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TimerContextState {
    watch_only: bool,
    view_changing: bool,
    commit_sent: bool,
    validator_threshold: usize,
    time_per_block: Duration,
}

impl TimerContextState {
    pub(crate) fn from_context(context: &mut ConsensusContext) -> Self {
        Self {
            watch_only: context.watch_only(),
            view_changing: context.view_changing(),
            commit_sent: context.commit_sent(),
            validator_threshold: context.m(),
            time_per_block: context.time_per_block,
        }
    }

    fn should_skip(&self) -> bool {
        self.watch_only || self.view_changing || self.commit_sent
    }
}
