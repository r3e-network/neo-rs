// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_service_check.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_service::{ConsensusService, TimerContextState};
use crate::dbft_plugin::types::change_view_reason::ChangeViewReason;

impl ConsensusService {
    /// Checks prepare response
    /// Matches C# CheckPrepareResponse method
    pub async fn check_prepare_response(&mut self) -> bool {
        let (payload, timer_state) = {
            let mut context = self.context.write().await;

            let ready = match (context.transaction_hashes(), context.transactions()) {
                (Some(hashes), Some(transactions)) => hashes.len() == transactions.len(),
                _ => false,
            };

            if !ready {
                return true;
            }

            if context.is_primary() || context.watch_only() {
                return true;
            }

            let block_index = context.block().index();

            if context.get_expected_block_size() > self.dbft_settings.max_block_size {
                self.log(&format!(
                    "Rejected block: {block_index} The size exceed the policy"
                ));
                drop(context);
                self.request_change_view(ChangeViewReason::BlockRejectedByPolicy)
                    .await;
                return false;
            }

            if context.get_expected_block_system_fee() > self.dbft_settings.max_block_system_fee {
                self.log(&format!(
                    "Rejected block: {block_index} The system fee exceed the policy"
                ));
                drop(context);
                self.request_change_view(ChangeViewReason::BlockRejectedByPolicy)
                    .await;
                return false;
            }

            let timer_state = TimerContextState::from_context(&mut context);
            let payload = context.make_prepare_response();
            (payload, timer_state)
        };

        self.extend_timer_by_factor(&timer_state, 2);

        self.extend_timer_async(2).await;

        self.log("Sending PrepareResponse");
        self.broadcast_payload(payload);
        self.check_preparations().await;
        true
    }

    /// Checks commits
    /// Matches C# CheckCommits method
    pub async fn check_commits(&mut self) {
        let ready = {
            let mut context = self.context.write().await;

            let payloads: Vec<_> = context
                .commit_payloads
                .iter()
                .filter_map(|payload| payload.clone())
                .collect();

            let mut commit_count = 0usize;
            for payload in &payloads {
                if let Some(commit) = context
                    .get_message(payload)
                    .and_then(|message| message.as_commit().cloned())
                {
                    if commit.view_number() == context.view_number() {
                        commit_count += 1;
                    }
                }
            }

            if commit_count < context.m() {
                return;
            }

            match (context.transaction_hashes(), context.transactions()) {
                (Some(hashes), Some(transactions))
                    if hashes.iter().all(|hash| transactions.contains_key(hash)) =>
                {
                    Some((context.block().index(), hashes.len()))
                }
                _ => None,
            }
        };

        if let Some((height, tx_count)) = ready {
            self.block_received_index = height;
            self.log(&format!(
                "Ready to send Block: height={height} tx={tx_count}"
            ));
            self.known_hashes.clear();
        }
    }

    /// Checks expected view
    /// Matches C# CheckExpectedView method
    pub async fn check_expected_view(&mut self, view_number: u8) {
        let agreement_payload = {
            let mut context = self.context.write().await;

            if context.view_number() >= view_number {
                return;
            }

            let payloads: Vec<_> = context
                .change_view_payloads
                .iter()
                .filter_map(|payload| payload.clone())
                .collect();

            let mut messages = Vec::with_capacity(payloads.len());
            for payload in &payloads {
                let message = context
                    .get_message(payload)
                    .and_then(|payload| payload.as_change_view().cloned());
                messages.push(message);
            }

            let valid_messages = messages
                .iter()
                .filter(|message| {
                    message
                        .as_ref()
                        .map(|m| m.new_view_number() >= view_number)
                        .unwrap_or(false)
                })
                .count();

            if valid_messages < context.m() {
                return;
            }

            let mut agreement_payload = None;
            if !context.watch_only() {
                let my_index = context.my_index;
                if my_index >= 0 {
                    let needs_agreement = messages
                        .get(my_index as usize)
                        .and_then(|message| message.as_ref().cloned())
                        .map(|msg| msg.new_view_number() < view_number)
                        .unwrap_or(true);

                    if needs_agreement {
                        agreement_payload =
                            Some(context.make_change_view(ChangeViewReason::ChangeAgreement));
                    }
                }
            }

            agreement_payload
        };

        if let Some(payload) = agreement_payload {
            self.broadcast_payload(payload);
        }

        self.initialize_consensus(view_number).await;
    }

    /// Checks preparations
    /// Matches C# CheckPreparations method
    pub async fn check_preparations(&mut self) {
        let action = {
            let mut context = self.context.write().await;

            let preparation_count = context.preparation_payloads.iter().flatten().count();

            if preparation_count < context.m() {
                return;
            }

            if let (Some(hashes), Some(transactions)) =
                (context.transaction_hashes(), context.transactions())
            {
                if hashes.iter().all(|hash| transactions.contains_key(hash)) {
                    let payload = context.make_commit();
                    context.save();
                    let delay = context.time_per_block;
                    Some((payload, delay))
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some((payload, delay)) = action {
            self.log("Sending Commit");
            self.broadcast_payload(payload);
            self.change_timer(delay);
            self.check_commits().await;
        }
    }
}
