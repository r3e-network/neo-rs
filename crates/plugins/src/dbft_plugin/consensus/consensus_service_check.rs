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

use crate::dbft_plugin::consensus::consensus_service::ConsensusService;
use crate::dbft_plugin::messages::change_view::ChangeView;
use crate::dbft_plugin::messages::commit::Commit;
use crate::dbft_plugin::types::change_view_reason::ChangeViewReason;

impl ConsensusService {
    /// Checks prepare response
    /// Matches C# CheckPrepareResponse method
    pub async fn check_prepare_response(&mut self) -> bool {
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

        self.extend_timer_by_factor(&context, 2);

        self.log("Sending PrepareResponse");
        let payload = context.make_prepare_response();
        let payload_to_send = payload.clone();
        drop(context);

        self.broadcast_payload(payload_to_send);
        self.check_preparations().await;
        true
    }

    /// Checks commits
    /// Matches C# CheckCommits method
    pub async fn check_commits(&mut self) {
        let proposal = {
            let mut context = self.context.write().await;

            let commit_count = context
                .commit_payloads
                .iter()
                .filter_map(|payload| payload.as_ref())
                .filter(|payload| {
                    context
                        .get_message(payload)
                        .and_then(|message| message.as_commit())
                        .map(|commit| commit.view_number() == context.view_number())
                        .unwrap_or(false)
                })
                .count();

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

        if let Some((height, tx_count)) = proposal {
            self.block_received_index = height;
            self.log(&format!("Ready to send Block: height={height} tx={tx_count}"));
            self.known_hashes.clear();
        }
    }

    /// Checks expected view
    /// Matches C# CheckExpectedView method
    pub async fn check_expected_view(&mut self, view_number: u8) {
        let (should_initialize, agreement_payload) = {
            let mut context = self.context.write().await;

            if context.view_number() >= view_number {
                return;
            }

            let messages: Vec<Option<ChangeView>> = context
                .change_view_payloads
                .iter()
                .map(|payload| {
                    payload
                        .as_ref()
                        .and_then(|p| context.get_message(p))
                        .and_then(|message| message.as_change_view().cloned())
                })
                .collect();

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
                    let current_message = messages
                        .get(my_index as usize)
                        .and_then(|message| message.as_ref().cloned());

                    if current_message
                        .map(|msg| msg.new_view_number() < view_number)
                        .unwrap_or(true)
                    {
                        agreement_payload =
                            Some(context.make_change_view(ChangeViewReason::ChangeAgreement));
                    }
                }
            }

            (true, agreement_payload)
        };

        if let Some(payload) = agreement_payload {
            self.broadcast_payload(payload);
        }

        if should_initialize {
            self.initialize_consensus(view_number).await;
        }
    }

    /// Checks preparations
    /// Matches C# CheckPreparations method
    pub async fn check_preparations(&mut self) {
        let action = {
            let mut context = self.context.write().await;

            let preparation_count = context
                .preparation_payloads
                .iter()
                .filter(|payload| payload.is_some())
                .count();

            if preparation_count >= context.m() {
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
