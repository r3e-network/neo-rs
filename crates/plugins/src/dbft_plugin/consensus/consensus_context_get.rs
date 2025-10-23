// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_context_get.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::dbft_plugin::consensus::consensus_context::ConsensusContext;
use crate::dbft_plugin::messages::ConsensusMessagePayload;
use crate::dbft_plugin::messages::recovery_message::{
    ChangeViewPayloadCompact, CommitPayloadCompact, PreparationPayloadCompact,
};
use neo_core::network::p2p::payloads::ExtensiblePayload;
use neo_core::smart_contract::Contract;
use neo_core::neo_io::Serializable;
use neo_core::{UInt160, UInt256};
use tracing::debug;

impl ConsensusContext {
    /// Retrieves (and caches) the consensus message embedded in the payload.
    pub fn get_message(&mut self, payload: &ExtensiblePayload) -> Option<ConsensusMessagePayload> {
        if payload.data.is_empty() {
            return None;
        }

        let mut payload_clone = payload.clone();
        let payload_hash = payload_clone.hash();

        if let Some(existing) = self.cached_messages().get(&payload_hash) {
            return Some(existing.clone());
        }

        match ConsensusMessagePayload::deserialize_from(&payload_clone.data) {
            Ok(message) => {
                self.cached_messages_mut().insert(payload_hash, message.clone());
                Some(message)
            }
            Err(error) => {
                debug!(
                    target: "dbft::consensus_context",
                    "Failed to deserialize consensus payload: {error}"
                );
                None
            }
        }
    }

    /// Builds a compact change-view payload matching the C# implementation.
    pub fn get_change_view_payload_compact(
        &mut self,
        payload: &ExtensiblePayload,
    ) -> Option<ChangeViewPayloadCompact> {
        let message = self.get_message(payload)?;
        message.as_change_view().map(|change_view| {
            ChangeViewPayloadCompact::new(
                change_view.header().validator_index,
                change_view.header().view_number,
                change_view.timestamp(),
                payload.witness.invocation_script.clone(),
            )
        })
    }

    /// Builds a compact commit payload matching the C# implementation.
    pub fn get_commit_payload_compact(
        &mut self,
        payload: &ExtensiblePayload,
    ) -> Option<CommitPayloadCompact> {
        let message = self.get_message(payload)?;
        message.as_commit().and_then(|commit| {
            CommitPayloadCompact::new(
                commit.header().view_number,
                commit.header().validator_index,
                commit.signature().to_vec(),
                payload.witness.invocation_script.clone(),
            )
            .ok()
        })
    }

    /// Builds a compact prepare payload matching the C# implementation.
    pub fn get_preparation_payload_compact(
        &mut self,
        payload: &ExtensiblePayload,
    ) -> Option<PreparationPayloadCompact> {
        let message = self.get_message(payload)?;
        let header = message.header();
        PreparationPayloadCompact::new(
            header.validator_index,
            payload.witness.invocation_script.clone(),
        )
        .ok()
    }

    /// Computes the expected block size including transactions.
    pub fn get_expected_block_size(&self) -> u32 {
        if let Some(transactions) = self.transactions() {
            self.get_expected_block_size_without_transactions(transactions.len())
                + transactions.values().map(|tx| tx.size() as u32).sum::<u32>()
        } else {
            self.get_expected_block_size_without_transactions(0)
        }
    }

    /// Computes the expected system fee for the proposed block.
    pub fn get_expected_block_system_fee(&self) -> i64 {
        if let Some(transactions) = self.transactions() {
            transactions.values().map(|tx| tx.system_fee()).sum()
        } else {
            0
        }
    }

    /// Computes the expected block size without considering transactions.
    pub fn get_expected_block_size_without_transactions(&self, expected_transactions: usize) -> u32 {
        4 + // Version
        32 + // PrevHash
        32 + // MerkleRoot
        8 + // Timestamp
        8 + // Nonce
        4 + // Index
        1 + // PrimaryIndex
        20 + // NextConsensus
        1 + self.witness_size() as u32 + // Witness size
        Self::get_var_size(expected_transactions) // Transaction count prefix
    }

    /// Retrieves the primary validator index for a view number.
    pub fn get_primary_index(&self, view_number: u8) -> u8 {
        let validators_len = self.validators().len() as i32;
        if validators_len == 0 {
            return 0;
        }

        let mut value = (self.block().index() as i32) - (view_number as i32);
        value %= validators_len;
        if value < 0 {
            (value + validators_len) as u8
        } else {
            value as u8
        }
    }

    /// Resolves the expected sender script hash for the specified validator index.
    pub fn get_sender(&self, index: usize) -> UInt160 {
        self.validators()
            .get(index)
            .map(|validator| Contract::create_signature_contract(validator.clone()).script_hash())
            .unwrap_or_default()
    }

    /// Helper translating C# GetVarSize.
    fn get_var_size(value: usize) -> u32 {
        if value < 0xFD {
            1
        } else if value <= 0xFFFF {
            3
        } else if value <= 0xFFFF_FFFF {
            5
        } else {
            9
        }
    }

    /// Computes the hash of an extensible payload (matches C# behaviour).
    pub fn payload_hash(payload: &mut ExtensiblePayload) -> UInt256 {
        payload.hash()
    }
}
