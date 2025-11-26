// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_service_on_message.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_context::ConsensusContext;
use crate::dbft_plugin::consensus::consensus_service::{ConsensusService, TimerContextState};
use crate::dbft_plugin::messages::{
    ChangeView, Commit, ConsensusMessagePayload, PrepareRequest, PrepareResponse, RecoveryMessage,
    RecoveryRequest,
};
use neo_core::cryptography::crypto_utils::Crypto;
use neo_core::ledger::TransactionVerificationContext;
use neo_core::network::p2p::payloads::ExtensiblePayload;
use neo_core::IVerifiable;
use std::collections::VecDeque;
use std::time::Instant;

impl ConsensusService {
    /// Handles consensus payload
    /// Matches C# OnConsensusPayload method
    pub async fn on_consensus_payload(&mut self, payload: ExtensiblePayload) {
        let mut queue = VecDeque::from([payload]);

        while let Some(next_payload) = queue.pop_front() {
            if let Some(follow_up) = self.handle_consensus_payload(next_payload).await {
                queue.extend(follow_up);
            }
        }
    }

    async fn handle_consensus_payload(
        &mut self,
        payload: ExtensiblePayload,
    ) -> Option<Vec<ExtensiblePayload>> {
        let message_payload = {
            let mut context = self.context.write().await;

            if context.block_sent() {
                return None;
            }

            let message = match context.get_message(&payload) {
                Some(msg) => msg,
                None => {
                    self.log("Failed to deserialize consensus message");
                    return None;
                }
            };

            if !message.verify(self.neo_system.settings()) {
                return None;
            }

            let current_height = context.block().index();
            let incoming_height = message.block_index();

            if incoming_height != current_height {
                if current_height < incoming_height {
                    self.log(&format!(
                        "Chain is behind: expected={} current={}",
                        incoming_height,
                        current_height.saturating_sub(1)
                    ));
                }
                return None;
            }

            let validator_index = message.validator_index() as usize;
            let validator = context.validators().get(validator_index).cloned()?;

            context
                .last_seen_message_mut()
                .insert(validator, message.block_index());

            message
        };

        match message_payload {
            ConsensusMessagePayload::PrepareRequest(request) => {
                self.on_prepare_request_received(payload.clone(), request)
                    .await;
                None
            }
            ConsensusMessagePayload::PrepareResponse(response) => {
                self.on_prepare_response_received(payload.clone(), response)
                    .await;
                None
            }
            ConsensusMessagePayload::ChangeView(change_view) => {
                self.on_change_view_received(payload.clone(), change_view)
                    .await;
                None
            }
            ConsensusMessagePayload::Commit(commit) => {
                self.on_commit_received(payload.clone(), commit).await;
                None
            }
            ConsensusMessagePayload::RecoveryRequest(recovery_request) => {
                self.on_recovery_request_received(payload, recovery_request)
                    .await;
                None
            }
            ConsensusMessagePayload::RecoveryMessage(recovery_message) => {
                Some(self.on_recovery_message_received(recovery_message).await)
            }
        }
    }

    async fn on_prepare_request_received(
        &mut self,
        payload: ExtensiblePayload,
        message: PrepareRequest,
    ) {
        let (missing_hashes, timer_state) = {
            let mut context = self.context.write().await;

            if context.request_sent_or_received()
                || context.not_accepting_payloads_due_to_view_changing()
            {
                return;
            }

            let block = context.block();

            if message.validator_index() != block.primary_index()
                || message.view_number() != context.view_number()
                || message.version() != block.version()
                || message.prev_hash() != block.prev_hash()
            {
                return;
            }

            if message.transaction_hashes().len()
                > self.neo_system.settings().max_transactions_per_block as usize
            {
                return;
            }

            self.prepare_request_received_time = Some(Instant::now());
            self.prepare_request_received_block_index = message.block_index();

            {
                let block_mut = context.block_mut();
                block_mut.header.set_timestamp(message.timestamp());
                block_mut.header.set_nonce(message.nonce());
            }

            let timer_state = TimerContextState::from_context(&mut context);
            context.transaction_hashes = Some(message.transaction_hashes().to_vec());
            context.transactions = Some(Default::default());
            context.verification_context = TransactionVerificationContext::new();

            for slot in &mut context.preparation_payloads {
                *slot = None;
            }
            for slot in &mut context.commit_payloads {
                *slot = None;
            }

            let primary_index = context.block().primary_index() as usize;
            if primary_index < context.preparation_payloads.len() {
                context.preparation_payloads[primary_index] = Some(payload.clone());
            }

            (message.transaction_hashes().to_vec(), timer_state)
        };

        self.extend_timer_by_factor(&timer_state, 2);

        if !missing_hashes.is_empty() {
            self.request_missing_transactions(&missing_hashes);
        }

        self.check_prepare_response().await;
    }

    async fn on_prepare_response_received(
        &mut self,
        payload: ExtensiblePayload,
        message: PrepareResponse,
    ) {
        let (should_check, timer_state) = {
            let mut context = self.context.write().await;

            if message.view_number() != context.view_number() {
                return;
            }

            if context.not_accepting_payloads_due_to_view_changing() {
                return;
            }

            let index = message.validator_index() as usize;
            if index >= context.preparation_payloads.len() {
                return;
            }

            if context.preparation_payloads[index].is_some() {
                return;
            }

            let primary_index = context.block().primary_index() as usize;
            let hash_matches = if let Some(Some(primary_payload)) =
                context.preparation_payloads.get(primary_index).cloned()
            {
                let mut payload_clone = primary_payload;
                let primary_hash = ConsensusContext::payload_hash(&mut payload_clone);
                message.preparation_hash() == &primary_hash
            } else {
                true
            };

            if !hash_matches {
                return;
            }

            let timer_state = TimerContextState::from_context(&mut context);

            self.log(&format!(
                "OnPrepareResponseReceived: height={} view={} index={}",
                message.block_index(),
                message.view_number(),
                message.validator_index()
            ));

            context.preparation_payloads[index] = Some(payload.clone());

            (
                !context.watch_only()
                    && !context.commit_sent()
                    && context.request_sent_or_received(),
                timer_state,
            )
        };

        self.extend_timer_by_factor(&timer_state, 2);

        if should_check {
            self.check_preparations().await;
        }
    }

    async fn on_change_view_received(&mut self, payload: ExtensiblePayload, message: ChangeView) {
        let expected_view = {
            let mut context = self.context.write().await;

            if message.view_number() != context.view_number() {
                return;
            }

            self.log(&format!(
                "OnChangeViewReceived: height={} view={} index={} nv={}",
                message.block_index(),
                message.view_number(),
                message.validator_index(),
                message.new_view_number()
            ));

            let index = message.validator_index() as usize;
            if index >= context.change_view_payloads.len() {
                return;
            }

            context.change_view_payloads[index] = Some(payload.clone());

            message.new_view_number()
        };

        self.check_expected_view(expected_view).await;
    }

    async fn on_commit_received(&mut self, payload: ExtensiblePayload, message: Commit) {
        let (should_check, timer_state) = {
            let mut context = self.context.write().await;

            if message.view_number() != context.view_number() {
                return;
            }

            let index = message.validator_index() as usize;
            if index >= context.commit_payloads.len() {
                return;
            }

            if context.commit_payloads[index].is_some() {
                return;
            }

            // SECURITY FIX: Verify the commit signature before accepting
            // The signature must be valid for the block being proposed
            let signature = message.signature();
            if signature.len() != 64 {
                self.log(&format!(
                    "OnCommitReceived: invalid signature length {} from validator {}",
                    signature.len(),
                    index
                ));
                return;
            }

            // Get the validator's public key
            let validator_pubkey = match context.validators.get(index) {
                Some(pk) => pk.clone(),
                None => {
                    self.log(&format!("OnCommitReceived: validator {} not found", index));
                    return;
                }
            };

            // Get the block header hash data for signature verification
            // The commit signature is over the unsigned header data
            let hash_data = context.block.header.get_hash_data();

            // Verify signature using secp256r1 (Neo's default curve)
            let pubkey_bytes = match validator_pubkey.encode_point(true) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.log(&format!(
                        "OnCommitReceived: failed to encode validator {} public key: {}",
                        index, e
                    ));
                    return;
                }
            };
            let mut sig_array = [0u8; 64];
            sig_array.copy_from_slice(signature);

            let is_valid =
                Crypto::verify_signature_secp256r1(&hash_data, &sig_array, &pubkey_bytes);

            if !is_valid {
                self.log(&format!(
                    "OnCommitReceived: INVALID signature from validator {} - rejecting commit",
                    index
                ));
                return;
            }

            let timer_state = TimerContextState::from_context(&mut context);

            self.log(&format!(
                "OnCommitReceived: height={} view={} index={} (signature verified)",
                message.block_index(),
                message.view_number(),
                message.validator_index()
            ));

            context.commit_payloads[index] = Some(payload.clone());

            (true, timer_state)
        };

        self.extend_timer_by_factor(&timer_state, 4);

        if should_check {
            self.check_commits().await;
        }
    }

    async fn on_recovery_request_received(
        &mut self,
        payload: ExtensiblePayload,
        message: RecoveryRequest,
    ) {
        let mut payload_for_hash = payload.clone();
        let payload_hash = ConsensusContext::payload_hash(&mut payload_for_hash);

        if !self.known_hashes.insert(payload_hash) {
            return;
        }

        let should_send = {
            let context = self.context.read().await;

            if message.view_number() != context.view_number() {
                return;
            }

            if context.watch_only() {
                return;
            }

            if context.commit_sent() {
                true
            } else {
                let my_index = context.my_index;
                if my_index < 0 {
                    false
                } else {
                    let validator_count = context.validators().len();
                    let f_plus_one = context.f() + 1;
                    (1..=f_plus_one).any(|offset| {
                        let candidate =
                            (message.validator_index() as usize + offset) % validator_count;
                        candidate == my_index as usize
                    })
                }
            }
        };

        if !should_send {
            return;
        }

        let recovery_payload = {
            let mut context = self.context.write().await;
            context.make_recovery_message()
        };

        self.broadcast_payload(recovery_payload);
    }

    async fn on_recovery_message_received(
        &mut self,
        message: RecoveryMessage,
    ) -> Vec<ExtensiblePayload> {
        self.is_recovering = true;

        let (
            change_view_payloads,
            prepare_request_payload,
            prepare_response_payloads,
            commit_payloads,
            expected_view,
        ) = {
            let mut context = self.context.write().await;

            let current_view = context.view_number();
            let message_view = message.view_number();

            if message_view > current_view && context.commit_sent() {
                self.is_recovering = false;
                return Vec::new();
            }

            self.log(&format!(
                "OnRecoveryMessageReceived: height={} view={} index={}",
                message.block_index(),
                message_view,
                message.validator_index()
            ));

            let change_view_payloads = if message_view > current_view {
                match message.get_change_view_payloads(&mut context) {
                    Ok(payloads) => payloads,
                    Err(error) => {
                        self.log(&format!("Failed to rebuild change-view payloads: {error}"));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };

            let (prepare_request_payload, prepare_response_payloads) = if message_view
                == current_view
                && !context.not_accepting_payloads_due_to_view_changing()
                && !context.commit_sent()
            {
                let request = message
                    .get_prepare_request_payload(&mut context)
                    .unwrap_or(None);
                let responses = message
                    .get_prepare_response_payloads(&mut context)
                    .unwrap_or_default();
                (request, responses)
            } else {
                (None, Vec::new())
            };

            let commit_payloads = if message_view <= current_view {
                match message.get_commit_payloads_from_recovery_message(&mut context) {
                    Ok(payloads) => payloads,
                    Err(error) => {
                        self.log(&format!("Failed to rebuild commit payloads: {error}"));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };

            (
                change_view_payloads,
                prepare_request_payload,
                prepare_response_payloads,
                commit_payloads,
                current_view.wrapping_add(1),
            )
        };

        let processed_change_views = change_view_payloads.len();
        let processed_prepare_responses = prepare_response_payloads.len();
        let processed_commits = commit_payloads.len();
        let processed_prepare_request = prepare_request_payload.is_some() as usize;

        let mut queued_payloads = Vec::with_capacity(
            processed_change_views
                + processed_prepare_responses
                + processed_commits
                + processed_prepare_request,
        );

        queued_payloads.extend(change_view_payloads);
        if let Some(payload) = prepare_request_payload {
            queued_payloads.push(payload);
        }
        queued_payloads.extend(prepare_response_payloads);
        queued_payloads.extend(commit_payloads);

        self.is_recovering = false;

        if processed_change_views
            + processed_prepare_responses
            + processed_commits
            + processed_prepare_request
            > 0
        {
            self.log(&format!(
                "Recovery finished: ChgView={} PrepReq={} PrepResp={} Commits={}",
                processed_change_views,
                processed_prepare_request,
                processed_prepare_responses,
                processed_commits
            ));
        }

        self.check_expected_view(expected_view).await;

        queued_payloads
    }
}
