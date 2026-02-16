use super::super::ConsensusService;
use super::super::helpers::{invocation_script_from_signature, signature_from_invocation_script};
use crate::context::ConsensusState;
use crate::messages::{
    ChangeViewMessage, ChangeViewPayloadCompact, CommitMessage, CommitPayloadCompact,
    ConsensusPayload, PreparationPayloadCompact, PrepareRequestMessage, PrepareResponseMessage,
    RecoveryMessage,
};
use crate::{ChangeViewReason, ConsensusError, ConsensusMessageType, ConsensusResult};
use tracing::{debug, info, warn};

impl ConsensusService {
    /// Handles `RecoveryRequest` message
    pub(in crate::service) fn on_recovery_request(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received RecoveryRequest"
        );

        // SECURITY: Verify the requestor's signature
        if payload.witness.is_empty() {
            warn!(
                validator = payload.validator_index,
                "RecoveryRequest missing witness"
            );
            return Err(ConsensusError::signature_failed(
                "RecoveryRequest missing witness",
            ));
        }
        let sign_data = self.dbft_sign_data(payload)?;
        if !self.verify_signature(&sign_data, &payload.witness, payload.validator_index) {
            warn!(
                validator = payload.validator_index,
                "RecoveryRequest signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "RecoveryRequest signature invalid",
            ));
        }

        if !self.should_send_recovery_response(payload.validator_index)? {
            return Ok(());
        }

        // Build and send recovery message with current state
        let recovery = self.build_recovery_message()?;

        let payload =
            self.create_payload(ConsensusMessageType::RecoveryMessage, recovery.serialize())?;
        self.broadcast(payload)?;

        Ok(())
    }

    /// Handles `RecoveryMessage`
    pub(in crate::service) fn on_recovery_message(
        &mut self,
        payload: &ConsensusPayload,
    ) -> ConsensusResult<()> {
        // Verify the payload signature
        // SECURITY: Require non-empty witness and valid signature
        if payload.witness.is_empty() {
            warn!(
                validator = payload.validator_index,
                "RecoveryMessage missing witness"
            );
            return Err(ConsensusError::signature_failed(
                "RecoveryMessage missing witness",
            ));
        }
        let sign_data = self.dbft_sign_data(payload)?;
        if !self.verify_signature(&sign_data, &payload.witness, payload.validator_index) {
            warn!(
                validator = payload.validator_index,
                "RecoveryMessage signature verification failed"
            );
            return Err(ConsensusError::signature_failed(
                "RecoveryMessage signature invalid",
            ));
        }

        debug!(
            block_index = self.context.block_index,
            validator = payload.validator_index,
            "Received RecoveryMessage"
        );

        // Validate block index matches
        if payload.block_index != self.context.block_index {
            debug!(
                expected = self.context.block_index,
                received = payload.block_index,
                "RecoveryMessage block index mismatch, ignoring"
            );
            return Ok(());
        }

        // Parse the recovery message
        let recovery = RecoveryMessage::deserialize(
            &payload.data,
            payload.block_index,
            payload.view_number,
            payload.validator_index,
        )?;

        // Validate the recovery message
        recovery.validate()?;

        info!(
            block_index = payload.block_index,
            view_number = payload.view_number,
            change_views = recovery.change_view_messages.len(),
            preparations = recovery.preparation_messages.len(),
            commits = recovery.commit_messages.len(),
            has_prepare_request = recovery.prepare_request_message.is_some(),
            "Applying recovery message state"
        );

        let message_view = payload.view_number;
        let commit_sent = self
            .context
            .my_index
            .and_then(|idx| self.context.commits.get(&idx))
            .is_some();

        if message_view > self.context.view_number {
            if commit_sent {
                return Ok(());
            }

            for cv in &recovery.change_view_messages {
                if cv.validator_index as usize >= self.context.validator_count() {
                    continue;
                }
                let Some(signature) = signature_from_invocation_script(&cv.invocation_script)
                else {
                    continue;
                };
                let msg = ChangeViewMessage::new(
                    payload.block_index,
                    cv.original_view_number,
                    cv.validator_index,
                    cv.timestamp,
                    ChangeViewReason::Timeout,
                );
                let recovered = ConsensusPayload {
                    network: self.network,
                    block_index: payload.block_index,
                    validator_index: cv.validator_index,
                    view_number: cv.original_view_number,
                    message_type: ConsensusMessageType::ChangeView,
                    data: msg.serialize(),
                    witness: signature.to_vec(),
                };
                self.reprocess_recovery_payload(recovered);
            }

            return Ok(());
        }

        if message_view == self.context.view_number
            && !commit_sent
            && !self.context.not_accepting_payloads_due_to_view_changing()
        {
            #[allow(clippy::collapsible_if)]
            if !self.context.prepare_request_received {
                if let Some(ref prep_req) = recovery.prepare_request_message {
                    let primary_index = self.context.primary_index();
                    if let Some(primary_prep) = recovery
                        .preparation_messages
                        .iter()
                        .find(|p| p.validator_index == primary_index)
                    {
                        if let Some(signature) =
                            signature_from_invocation_script(&primary_prep.invocation_script)
                        {
                            let recovered = ConsensusPayload {
                                network: self.network,
                                block_index: prep_req.block_index,
                                validator_index: prep_req.validator_index,
                                view_number: prep_req.view_number,
                                message_type: ConsensusMessageType::PrepareRequest,
                                data: prep_req.serialize(),
                                witness: signature.to_vec(),
                            };
                            self.reprocess_recovery_payload(recovered);
                        }
                    }
                }
            }

            #[allow(clippy::collapsible_if)]
            if self.context.preparation_hash.is_none() {
                if let Some(hash) = recovery.preparation_hash {
                    self.context.preparation_hash = Some(hash);
                }
            }

            let primary_index = self.context.primary_index();
            let prep_hash = recovery.preparation_hash.or(self.context.preparation_hash);
            if let Some(prep_hash) = prep_hash {
                for prep in &recovery.preparation_messages {
                    if prep.validator_index as usize >= self.context.validator_count() {
                        continue;
                    }
                    if prep.validator_index == primary_index {
                        continue;
                    }
                    let Some(signature) = signature_from_invocation_script(&prep.invocation_script)
                    else {
                        continue;
                    };

                    let msg = PrepareResponseMessage::new(
                        payload.block_index,
                        message_view,
                        prep.validator_index,
                        prep_hash,
                    );
                    let recovered = ConsensusPayload {
                        network: self.network,
                        block_index: payload.block_index,
                        validator_index: prep.validator_index,
                        view_number: message_view,
                        message_type: ConsensusMessageType::PrepareResponse,
                        data: msg.serialize(),
                        witness: signature.to_vec(),
                    };
                    self.reprocess_recovery_payload(recovered);
                }
            }
        }

        if message_view <= self.context.view_number {
            for commit in &recovery.commit_messages {
                if commit.validator_index as usize >= self.context.validator_count() {
                    continue;
                }
                let Some(signature) = signature_from_invocation_script(&commit.invocation_script)
                else {
                    continue;
                };
                let msg = CommitMessage::new(
                    payload.block_index,
                    commit.view_number,
                    commit.validator_index,
                    commit.signature.clone(),
                );
                let recovered = ConsensusPayload {
                    network: self.network,
                    block_index: payload.block_index,
                    validator_index: commit.validator_index,
                    view_number: commit.view_number,
                    message_type: ConsensusMessageType::Commit,
                    data: msg.serialize(),
                    witness: signature.to_vec(),
                };
                self.reprocess_recovery_payload(recovered);
            }
        }

        // Check if we can now commit after applying recovery state
        if self.context.has_enough_commits() && self.context.state != ConsensusState::Committed {
            info!(
                block_index = self.context.block_index,
                commits = self.context.commits.len(),
                "Recovery enabled block commit"
            );
            self.check_commits()?;
        }
        // Check if we can now send commit after applying recovery state
        else if self.context.has_enough_prepare_responses()
            && !self
                .context
                .commits
                .contains_key(&self.context.my_index.unwrap_or(255))
        {
            if let Some(my_idx) = self.context.my_index {
                info!(
                    block_index = self.context.block_index,
                    "Recovery enabled sending commit"
                );
                // Create and broadcast commit message
                let block_hash = self.context.proposed_block_hash.unwrap_or_default();
                let signature = self.sign_block_hash(&block_hash)?;

                let commit = CommitMessage::new(
                    self.context.block_index,
                    self.context.view_number,
                    my_idx,
                    signature.clone(),
                );

                let payload =
                    self.create_payload(ConsensusMessageType::Commit, commit.serialize())?;
                let commit_witness = payload.witness.clone();
                let commit_invocation = invocation_script_from_signature(&commit_witness);
                self.broadcast(payload)?;
                if !commit_witness.is_empty() {
                    self.context
                        .commit_invocations
                        .insert(my_idx, commit_invocation);
                }

                // Add our own commit
                self.context
                    .add_commit(my_idx, self.context.view_number, signature)?;
                self.check_commits()?;
            }
        }

        Ok(())
    }

    pub(in crate::service) fn reprocess_recovery_payload(&mut self, payload: ConsensusPayload) {
        let result = match payload.message_type {
            ConsensusMessageType::ChangeView => self.on_change_view(&payload),
            ConsensusMessageType::PrepareRequest => self.on_prepare_request(&payload),
            ConsensusMessageType::PrepareResponse => self.on_prepare_response(&payload),
            ConsensusMessageType::Commit => self.on_commit(&payload),
            _ => Ok(()),
        };
        if let Err(err) = result {
            debug!(error = %err, msg_type = ?payload.message_type, "Ignored recovery payload");
        }
    }

    /// Determines whether this node should respond with a `RecoveryMessage`.
    ///
    /// Mirrors C# `DBFTPlugin` behavior:
    /// - If we've already sent a commit, always respond.
    /// - Otherwise, only `f + 1` nodes respond, selected by validator index rotation.
    pub(in crate::service) fn should_send_recovery_response(
        &self,
        requester_index: u8,
    ) -> ConsensusResult<bool> {
        let my_index = self.my_index()?;
        if self.context.commits.contains_key(&my_index) {
            return Ok(true);
        }

        let validators = self.context.validator_count();
        if validators == 0 {
            return Ok(false);
        }

        let allowed = self.context.f() + 1;
        for offset in 1..=allowed {
            let chosen = ((requester_index as usize + offset) % validators) as u8;
            if chosen == my_index {
                return Ok(true);
            }
        }

        Ok(false)
    }

    pub(in crate::service) fn maybe_send_recovery_response(
        &mut self,
        requester_index: u8,
    ) -> ConsensusResult<()> {
        if !self.should_send_recovery_response(requester_index)? {
            return Ok(());
        }

        let recovery = self.build_recovery_message()?;

        let payload =
            self.create_payload(ConsensusMessageType::RecoveryMessage, recovery.serialize())?;
        self.broadcast(payload)?;
        Ok(())
    }

    pub(in crate::service) fn build_recovery_message(&self) -> ConsensusResult<RecoveryMessage> {
        let mut recovery = RecoveryMessage::new(
            self.context.block_index,
            self.context.view_number,
            self.my_index()?,
        );

        let view_number = self.context.view_number;
        let change_view_limit = self.context.m();
        let mut change_views: Vec<ChangeViewPayloadCompact> = self
            .context
            .change_views
            .iter()
            .filter_map(|(&validator_index, &(new_view, _reason))| {
                if new_view < view_number {
                    return None;
                }
                let original_view_number = new_view.saturating_sub(1);
                let timestamp = self
                    .context
                    .last_change_view_timestamps
                    .get(&validator_index)
                    .copied()
                    .unwrap_or(0);
                let invocation_script = self
                    .context
                    .change_view_invocations
                    .get(&validator_index)
                    .cloned()
                    .unwrap_or_default();
                Some(ChangeViewPayloadCompact {
                    validator_index,
                    original_view_number,
                    timestamp,
                    invocation_script,
                })
            })
            .collect();
        change_views.sort_by_key(|p| p.validator_index);
        for payload in change_views.into_iter().take(change_view_limit) {
            recovery.change_view_messages.push(payload);
        }

        let primary_index = self.context.primary_index();
        let has_prepare_request_invocation = self
            .context
            .prepare_request_invocation
            .as_ref()
            .is_some_and(|inv| !inv.is_empty());
        let include_prepare_request =
            self.context.prepare_request_received && has_prepare_request_invocation;

        if include_prepare_request {
            let prepare_request = PrepareRequestMessage::new(
                self.context.block_index,
                self.context.view_number,
                primary_index,
                self.context.version,
                self.context.prev_hash,
                self.context.proposed_timestamp,
                self.context.nonce,
                self.context.proposed_tx_hashes.clone(),
            );
            recovery.prepare_request_message = Some(prepare_request);

            if let Some(invocation_script) = self.context.prepare_request_invocation.clone() {
                recovery
                    .preparation_messages
                    .push(PreparationPayloadCompact {
                        validator_index: primary_index,
                        invocation_script,
                    });
            }
        } else {
            let mut counts = std::collections::HashMap::new();
            for hash in self.context.prepare_response_hashes.values() {
                *counts.entry(*hash).or_insert(0usize) += 1;
            }
            let majority = counts
                .into_iter()
                .max_by_key(|(_, count)| *count)
                .map(|(hash, _)| hash);
            if let Some(hash) = majority.or(self.context.preparation_hash) {
                recovery.preparation_hash = Some(hash);
            }
        }

        for (&validator_index, invocation_script) in &self.context.prepare_responses {
            if validator_index == primary_index && include_prepare_request {
                continue;
            }
            recovery
                .preparation_messages
                .push(PreparationPayloadCompact {
                    validator_index,
                    invocation_script: invocation_script.clone(),
                });
        }

        let commit_sent = self
            .context
            .my_index
            .and_then(|idx| self.context.commits.get(&idx))
            .is_some();
        if commit_sent {
            for (&validator_index, signature) in &self.context.commits {
                let invocation_script = self
                    .context
                    .commit_invocations
                    .get(&validator_index)
                    .cloned()
                    .unwrap_or_default();
                let commit_view = self
                    .context
                    .commit_view_numbers
                    .get(&validator_index)
                    .copied()
                    .unwrap_or(self.context.view_number);
                recovery.commit_messages.push(CommitPayloadCompact {
                    view_number: commit_view,
                    validator_index,
                    signature: signature.clone(),
                    invocation_script,
                });
            }
        }

        Ok(recovery)
    }
}
