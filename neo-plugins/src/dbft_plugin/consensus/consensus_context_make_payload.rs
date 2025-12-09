// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_context_make_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_context::ConsensusContext;
use crate::dbft_plugin::messages::{
    ChangeView, Commit, ConsensusMessagePayload, PrepareRequest, PrepareResponse, RecoveryMessage,
    RecoveryRequest,
};
use crate::dbft_plugin::types::change_view_reason::ChangeViewReason;
use neo_core::ledger::TransactionVerificationContext;
use neo_core::neo_io::Serializable;
use neo_core::network::p2p::payloads::ExtensiblePayload;
use neo_core::network::p2p::payloads::Witness;
use neo_core::smart_contract::Contract;
use neo_core::{TimeProvider, Transaction, UInt160};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

impl ConsensusContext {
    /// Makes a change view payload
    /// Matches C# MakeChangeView method
    pub fn make_change_view(&mut self, reason: ChangeViewReason) -> ExtensiblePayload {
        if self.watch_only() {
            return ExtensiblePayload::new();
        }
        let change_view = ChangeView::with_params(
            self.block.index(),
            self.my_index as u8,
            self.view_number,
            TimeProvider::current().utc_now().timestamp_millis() as u64,
            reason,
        );
        let payload = self.make_signed_payload(ConsensusMessagePayload::ChangeView(change_view));
        if self.my_index >= 0 {
            self.change_view_payloads[self.my_index as usize] = Some(payload.clone());
        }
        payload
    }

    /// Makes a commit payload
    /// Matches C# MakeCommit method
    pub fn make_commit(&mut self) -> ExtensiblePayload {
        if self.watch_only() {
            return ExtensiblePayload::new();
        }
        if let Some(existing) = &self.commit_payloads[self.my_index as usize] {
            return existing.clone();
        }
        if self.my_public_key.is_none() {
            self.log("Commit requested without validator key");
            return ExtensiblePayload::new();
        }

        self.ensure_header();
        let public_key = self.my_public_key.as_ref().unwrap();
        let signature =
            match self
                .signer
                .sign_block(&self.block, public_key, self.dbft_settings.network)
            {
                Ok(sig) => sig,
                Err(err) => {
                    self.log(&format!("Failed to sign commit: {err}"));
                    return ExtensiblePayload::new();
                }
            };

        let commit = Commit::with_params(
            self.block.index(),
            self.my_index as u8,
            self.view_number,
            signature,
        );

        let payload = self.make_signed_payload(ConsensusMessagePayload::Commit(commit));
        self.commit_payloads[self.my_index as usize] = Some(payload.clone());
        payload
    }

    /// Makes a signed payload
    /// Matches C# MakeSignedPayload method
    fn make_signed_payload(&mut self, message: ConsensusMessagePayload) -> ExtensiblePayload {
        let mut payload = self.create_payload(message, None);
        self.sign_payload(&mut payload);
        payload
    }

    /// Signs a payload
    /// Matches C# SignPayload method
    fn sign_payload(&self, payload: &mut ExtensiblePayload) {
        match self.signer.sign_extensible_payload(
            payload,
            &self.data_cache,
            self.dbft_settings.network,
        ) {
            Ok(witness) => {
                payload.witness = witness;
            }
            Err(ex) => {
                self.log(&format!("SignPayload error: {}", ex));
            }
        }
    }

    /// Ensures max block limitation
    /// Matches C# EnsureMaxBlockLimitation method
    pub fn ensure_max_block_limitation(&mut self, txs: Vec<Transaction>) {
        let mut hashes = Vec::new();
        self.transactions = Some(HashMap::new());
        self.verification_context = TransactionVerificationContext::new();

        // Expected block size
        let mut block_size = self.get_expected_block_size_without_transactions(txs.len());
        let mut block_system_fee = 0i64;

        // Iterate transaction until reach the size or maximum system fee
        for tx in txs {
            // Check if maximum block size has been already exceeded with the current selected set
            block_size = block_size.saturating_add(tx.size() as u32);
            if block_size > self.dbft_settings.max_block_size {
                break;
            }

            // Check if maximum block system fee has been already exceeded with the current selected set
            block_system_fee += tx.system_fee();
            if block_system_fee > self.dbft_settings.max_block_system_fee {
                break;
            }

            hashes.push(tx.hash());
            if let Some(transactions) = &mut self.transactions {
                transactions.insert(tx.hash(), tx.clone());
            }
            self.verification_context.add_transaction(&tx);
        }

        self.transaction_hashes = Some(hashes);
    }

    /// Makes a prepare request payload
    /// Matches C# MakePrepareRequest method
    pub fn make_prepare_request(&mut self) -> ExtensiblePayload {
        let max_transactions_per_block = self.neo_system.settings().max_transactions_per_block;
        // Limit Speaker proposal to the limit `MaxTransactionsPerBlock` or all available transactions of the mempool
        let sorted_txs = self.collect_transactions(max_transactions_per_block as usize);
        self.ensure_max_block_limitation(sorted_txs);

        let timestamp = std::cmp::max(
            TimeProvider::current().utc_now().timestamp_millis() as u64,
            self.prev_header().map(|h| h.timestamp + 1).unwrap_or(0),
        );
        self.block.header.set_timestamp(timestamp);
        let nonce = self.get_nonce();
        self.block.header.set_nonce(nonce);

        let prepare_request = PrepareRequest::with_params(
            self.block.index(),
            self.my_index as u8,
            self.view_number,
            self.block.version(),
            *self.block.prev_hash(),
            timestamp,
            nonce,
            self.transaction_hashes.clone().unwrap_or_default(),
        );

        let payload =
            self.make_signed_payload(ConsensusMessagePayload::PrepareRequest(prepare_request));
        self.preparation_payloads[self.my_index as usize] = Some(payload.clone());
        payload
    }

    /// Makes a recovery request payload
    /// Matches C# MakeRecoveryRequest method
    pub fn make_recovery_request(&mut self) -> ExtensiblePayload {
        let recovery_request = RecoveryRequest::with_params(
            self.block.index(),
            self.my_index as u8,
            self.view_number,
            TimeProvider::current().utc_now().timestamp_millis() as u64,
        );

        self.make_signed_payload(ConsensusMessagePayload::RecoveryRequest(recovery_request))
    }

    /// Makes a recovery message payload
    /// Matches C# MakeRecoveryMessage method
    pub fn make_recovery_message(&mut self) -> ExtensiblePayload {
        let mut prepare_request_message = None;
        if let Some(transaction_hashes) = &self.transaction_hashes {
            prepare_request_message = Some(PrepareRequest::with_params(
                self.block.index(),
                self.block.primary_index(),
                self.view_number,
                self.block.version(),
                *self.block.prev_hash(),
                self.block.header.timestamp(),
                self.block.header.nonce(),
                transaction_hashes.clone(),
            ));
        }

        let mut change_view_messages = HashMap::new();
        let change_payloads: Vec<ExtensiblePayload> = self
            .last_change_view_payloads
            .iter()
            .filter_map(|payload| payload.clone())
            .collect();
        for payload in change_payloads {
            if change_view_messages.len() >= self.m() {
                break;
            }
            if let Some(compact) = self.get_change_view_payload_compact(&payload) {
                change_view_messages.insert(compact.validator_index, compact);
            }
        }

        let mut preparation_hash = None;
        if self.transaction_hashes.is_none() {
            let preparation_payloads: Vec<ExtensiblePayload> = self
                .preparation_payloads
                .iter()
                .filter_map(|payload| payload.clone())
                .collect();
            for payload in preparation_payloads {
                if let Some(message) = self.get_message(&payload) {
                    if let Some(response) = message.as_prepare_response() {
                        let candidate = *response.preparation_hash();
                        if preparation_hash
                            .as_ref()
                            .is_some_and(|current| current >= &candidate)
                        {
                            continue;
                        }
                        preparation_hash = Some(candidate);
                    }
                }
            }
        }

        let mut preparation_messages = HashMap::new();
        let preparation_payloads: Vec<ExtensiblePayload> = self
            .preparation_payloads
            .iter()
            .filter_map(|payload| payload.clone())
            .collect();
        for payload in preparation_payloads {
            if let Some(compact) = self.get_preparation_payload_compact(&payload) {
                preparation_messages.insert(compact.validator_index, compact);
            }
        }

        let mut commit_messages = HashMap::new();
        if self.commit_sent() {
            let commit_payloads: Vec<ExtensiblePayload> = self
                .commit_payloads
                .iter()
                .filter_map(|payload| payload.clone())
                .collect();
            for payload in commit_payloads {
                if let Some(compact) = self.get_commit_payload_compact(&payload) {
                    commit_messages.insert(compact.validator_index, compact);
                }
            }
        }

        let recovery_message = RecoveryMessage::with_params(
            self.block.index(),
            self.my_index as u8,
            self.view_number,
            change_view_messages,
            prepare_request_message,
            preparation_hash,
            preparation_messages,
            commit_messages,
        );

        self.make_signed_payload(ConsensusMessagePayload::RecoveryMessage(recovery_message))
    }

    /// Makes a prepare response payload
    /// Matches C# MakePrepareResponse method
    pub fn make_prepare_response(&mut self) -> ExtensiblePayload {
        if self.watch_only() {
            return ExtensiblePayload::new();
        }

        let preparation_hash = self
            .preparation_payloads
            .get(self.block.primary_index() as usize)
            .and_then(|payload| payload.clone())
            .map(|mut payload| payload.hash())
            .unwrap_or_default();

        let prepare_response = PrepareResponse::with_params(
            self.block.index(),
            self.my_index as u8,
            self.view_number,
            preparation_hash,
        );

        let payload =
            self.make_signed_payload(ConsensusMessagePayload::PrepareResponse(prepare_response));
        self.preparation_payloads[self.my_index as usize] = Some(payload.clone());
        payload
    }

    /// Gets a nonce for the block
    /// Matches C# GetNonce method
    fn get_nonce(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .hash(&mut hasher);
        hasher.finish()
    }

    /// Logs a message
    fn log(&self, message: &str) {
        debug!(target: "dbft::consensus_context", "{}", message);
    }

    pub(crate) fn create_payload(
        &mut self,
        mut message: ConsensusMessagePayload,
        invocation_script: Option<Vec<u8>>,
    ) -> ExtensiblePayload {
        let validator_index = if self.my_index >= 0 {
            self.my_index as u8
        } else {
            0
        };

        {
            let header = message.header_mut();
            header.block_index = self.block.index();
            header.validator_index = validator_index;
            header.view_number = self.view_number;
        }

        let data = message.to_bytes().unwrap_or_else(|_| Vec::new());

        let mut payload = ExtensiblePayload::new();
        payload.category = "dBFT".to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = self.block.index();
        payload.sender = if self.my_index >= 0 {
            self.get_sender(self.my_index as usize)
        } else {
            UInt160::default()
        };
        payload.data = data;
        payload.witness = Witness::new();

        let validator_key = message.header().validator_index as usize;
        if let Some(script) = invocation_script {
            payload.witness.invocation_script = script;
        }
        if let Some(validator) = self.validators.get(validator_key) {
            payload.witness.verification_script =
                Contract::create_signature_redeem_script(validator.clone());
        }

        let mut hashable = payload.clone();
        let payload_hash = hashable.hash();
        self.cached_messages_mut().put(payload_hash, message);

        payload
    }

    fn collect_transactions(&self, limit: usize) -> Vec<Transaction> {
        if limit == 0 {
            return Vec::new();
        }

        match self.neo_system.mempool().lock() {
            Ok(pool) => pool.get_sorted_verified_transactions(limit),
            Err(err) => {
                debug!("ConsensusContext: failed to acquire mempool lock: {}", err);
                Vec::new()
            }
        }
    }
}
