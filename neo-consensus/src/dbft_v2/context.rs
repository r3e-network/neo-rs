// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{vec, vec::Vec};

use neo_base::{byzantine_honest_quorum};
use neo_core::{
    PublicKey, types::H256,
    tx::{Tx, Witness}, merkle::MerkleSha256,
    contract::{ToMultiSignContract, context::MultiSignContext},
};
use crate::dbft_v2::message::*;


#[derive(Debug, Default, Clone)]
pub struct ConsensusStates {
    pub(crate) prev_hash: H256,
    pub(crate) block_index: u32,
    pub(crate) view_number: ViewNumber,

    pub(crate) self_index: u8,
    pub(crate) not_validator: bool,
    pub(crate) watch_only: bool,

    /// block_index if block_index < view_number else (block_index - view_number) % validators.len()
    pub(crate) primary_index: u8,

    pub(crate) prev_block_unix_milli: u64,
}


impl ConsensusStates {
    pub fn new() -> Self {
        Self {
            prev_hash: Default::default(),
            block_index: 0,
            view_number: 0,
            self_index: 0,
            not_validator: false,
            watch_only: false,
            primary_index: 0,
            prev_block_unix_milli: 0,
        }
    }

    pub fn reset(&mut self, view_number: ViewNumber) {
        self.view_number = view_number;
        // TODO
    }

    pub fn is_primary(&self) -> bool {
        self.self_index == self.primary_index && !self.is_backup()
    }

    pub fn is_backup(&self) -> bool { self.not_validator || self.watch_only }

    pub fn new_message_meta(&self) -> MessageMeta {
        MessageMeta {
            block_index: self.block_index,
            validator_index: self.self_index,
            view_number: self.view_number,
        }
    }
}


pub struct ConsensusContext {
    pub(crate) tx_hashes: Vec<H256>,
    pub(crate) txs: hashbrown::HashMap<H256, Tx>,

    pub(crate) preparations: Vec<Option<(MessageMeta, PrepareRequest)>>,
    pub(crate) commits: Vec<Option<(MessageMeta, Commit)>>,
    pub(crate) change_views: Vec<Option<(MessageMeta, ChangeViewRequest)>>,
    pub(crate) last_change_views: Vec<Option<(MessageMeta, ChangeViewRequest)>>,

    /// public-key -> block height
    pub(crate) last_seen_message: hashbrown::HashMap<PublicKey, u32>,
}


impl ConsensusContext {
    pub fn new(nr_validators: u32) -> Self {
        let nr_validators = nr_validators as usize;
        Self {
            tx_hashes: Default::default(),
            txs: Default::default(),
            preparations: vec![None; nr_validators],
            commits: vec![None; nr_validators],
            change_views: vec![None; nr_validators],
            last_change_views: vec![None; nr_validators],
            last_seen_message: Default::default(),
        }
    }

    pub fn reset(&mut self, nr_validators: u32) {
        *self = Self::new(nr_validators)
        // TODO
    }

    pub fn nr_validators(&self) -> usize { self.preparations.len() }

    pub fn merkle_root(&self) -> H256 { self.tx_hashes.merkle_sha256() }

    pub fn has_commit(&self, index: ViewIndex) -> bool {
        let index = index as usize;
        index < self.commits.len() && self.commits[index].is_some()
    }

    pub fn commit_count(&self) -> usize {
        self.commits.iter().filter(|f| f.is_some()).count()
    }

    /// NOTE: block_index must greater than 0
    pub fn failed_count(&self, block_index: u32, validators: &[PublicKey]) -> usize {
        validators.iter()
            .map(|key| self.last_seen_message.get(key))
            .filter(|index| index.map(|&v| v < block_index - 1).unwrap_or(true))
            .count()
    }


    pub fn txs(&self) -> Vec<Tx> {
        self.tx_hashes.iter()
            .map(|h| self.txs.get(h).expect("tx should exists").clone())
            .collect()
    }

    pub fn new_block_witness(&self, view_number: ViewNumber, validators: &[PublicKey]) -> Witness {
        let signers = byzantine_honest_quorum(validators.len() as u32);
        let contract = validators.to_multi_sign_contract(signers)
            .expect("`validators` should be valid");

        let mut sign_cx = MultiSignContext::new(validators);
        for (idx, validator) in validators.iter().enumerate() {
            if let Some((meta, message)) = self.commits[idx].as_ref() {
                if meta.view_number != view_number {
                    continue;
                }

                let _ = sign_cx.add_sign(validator, &message.sign);
                if sign_cx.signs_count() >= signers as usize {
                    break;
                }
            }
        }

        let invocation = sign_cx.to_invocation_script();
        Witness::new(invocation, contract.script)
    }
}