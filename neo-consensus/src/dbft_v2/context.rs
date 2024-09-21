// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{vec, vec::Vec};
#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
use neo_base::byzantine_honest_quorum;
use neo_core::contract::{context::MultiSignContext, ToMultiSignContract};
use neo_core::tx::{Tx, Witness};
use neo_core::types::{H160, H256};
use neo_core::PublicKey;

use crate::dbft_v2::*;

#[derive(Debug, Default)]
pub struct ConsensusStates {
    pub validators: Vec<PublicKey>,
    pub prev_hash: H256,
    pub block_index: u32,
    pub view_number: ViewNumber,

    pub self_index: ViewIndex,
    pub not_validator: bool,
    pub watch_only: bool,

    pub on_recovering: bool,
    pub block_sent: bool,

    pub primary_index: ViewIndex,
    pub received_unix_milli: u64,
    pub received_block_index: u32,
}

impl ConsensusStates {
    pub fn new() -> Self {
        Self {
            validators: vec![],
            prev_hash: Default::default(),
            block_index: 0,
            view_number: 0,
            self_index: 0,
            not_validator: true,
            watch_only: true,
            on_recovering: false,
            block_sent: false,
            primary_index: 0,
            received_unix_milli: 0,
            received_block_index: 0,
        }
    }

    pub fn nr_validators(&self) -> usize { self.validators.len() }

    pub fn is_primary(&self) -> bool { self.self_index == self.primary_index && !self.is_backup() }

    pub fn is_backup(&self) -> bool { self.not_validator || self.watch_only }

    pub fn height_view(&self) -> HView {
        HView { height: self.block_index, view_number: self.view_number }
    }

    pub fn new_message_meta(&self) -> MessageMeta {
        MessageMeta {
            block_index: self.block_index,
            validator_index: self.self_index,
            view_number: self.view_number,
        }
    }
}

pub fn primary_index(block_index: u32, view_number: ViewNumber, nr_validators: u32) -> ViewIndex {
    let nr_validators = nr_validators as i64;
    let primary = (block_index as i64 - view_number as i64) % nr_validators;
    if primary >= 0 { primary as ViewIndex } else { (primary + nr_validators) as ViewIndex }
}

#[derive(Debug, Default, Clone)]
#[allow(dead_code)]
pub struct VerificationContext {
    pub senders_fee: HashMap<H160, u64>,
    pub oracle_responses: HashMap<u64, H256>,
}

impl VerificationContext {
    #[inline]
    pub fn new() -> Self { Self::default() }
}

#[derive(Debug, Default, Clone)]
pub struct Prepares {
    pub request: Option<Message<PrepareRequest>>,
    pub response: Option<Message<PrepareResponse>>,
}

impl Prepares {
    #[inline]
    pub fn has_request(&self) -> bool { self.request.is_some() }

    #[inline]
    pub fn has_response(&self) -> bool { self.response.is_some() }

    pub fn to_preparation_compact(&self) -> Option<PreparationCompact> {
        if let Some(res) = self.response.as_ref() {
            Some(res.to_preparation_compact())
        } else if let Some(req) = self.request.as_ref() {
            Some(req.to_preparation_compact())
        } else {
            None
        }
    }
}

pub struct ConsensusContext {
    pub tx_hashes: Vec<H256>,
    pub txs: HashMap<H256, Tx>,

    pub prepares: Vec<Prepares>,
    pub commits: Vec<Option<Message<Commit>>>,
    pub change_views: Vec<Option<Message<ChangeViewRequest>>>,
    pub last_change_views: Vec<Option<Message<ChangeViewRequest>>>,

    pub last_seen_message: HashMap<PublicKey, HView>,
    pub verifications: VerificationContext,
}

impl ConsensusContext {
    pub fn new(nr_validators: u32) -> Self {
        let nr_validators = nr_validators as usize;
        Self {
            tx_hashes: Vec::default(),
            txs: HashMap::default(),
            prepares: vec![Prepares::default(); nr_validators],
            commits: vec![None; nr_validators],
            change_views: vec![None; nr_validators],
            last_change_views: vec![None; nr_validators],
            last_seen_message: HashMap::new(),
            verifications: VerificationContext::new(),
        }
    }

    pub(crate) fn on_prepare_received(&mut self, prepare: Message<PrepareRequest>) -> H256 {
        let message = &prepare.message;

        self.tx_hashes = message.tx_hashes.clone();
        self.txs = HashMap::new(); // TODO: log the rewritten txs
        self.verifications = VerificationContext::new();
        for prepares in self.prepares.iter_mut() {
            let Some(r) = prepares.response.as_ref() else {
                continue;
            };
            if r.message.preparation != message.payload_hash {
                prepares.response = None;
            }
        }

        let payload_hash = prepare.message.payload_hash;
        let validator = prepare.meta.validator_index as usize; // validator == primary has checked
        self.prepares[validator].request = Some(prepare); // out-of-bound has checked

        payload_hash
    }

    pub fn has_commit(&self, index: ViewIndex) -> bool {
        let index = index as usize;
        index < self.commits.len() && self.commits[index].is_some()
    }

    pub fn has_preparation(&self, primary: ViewIndex) -> bool {
        let index = primary as usize;
        index < self.prepares.len()
            && (self.prepares[index].has_request() || self.prepares[index].has_response())
    }

    pub fn commit_count(&self) -> usize { self.commits.iter().filter(|f| f.is_some()).count() }

    /// NOTE: block_index must greater than 0
    pub fn failed_count(&self, block_index: u32, validators: &[PublicKey]) -> usize {
        validators
            .iter()
            .filter_map(|key| self.last_seen_message.get(key).cloned())
            .filter(|hv| hv.height < block_index - 1)
            .count()
    }

    pub fn txs(&self) -> Vec<Tx> {
        self.tx_hashes.iter().map_while(|h| self.txs.get(h).cloned()).collect()
    }

    pub fn has_all_txs(&self) -> bool { self.tx_hashes.iter().all(|tx| self.txs.contains_key(tx)) }

    fn max_quorum_preparation(&self) -> Option<H256> {
        let mut hashes = HashMap::new();
        self.prepares.iter().filter_map(|p| p.response.as_ref()).for_each(|res| {
            let hash = &res.message.preparation;
            hashes.insert(hash, 1u32 + hashes.get(&hash).cloned().unwrap_or(0));
        });
        hashes.into_iter().max_by(|x, y| x.1.cmp(&y.1)).map(|v| v.0.clone())
    }

    pub fn new_recovery_message(&self, meta: MessageMeta) -> Message<RecoveryMessage> {
        // let honest = byzantine_honest_quorum(self.nr_validators() as u32) as usize;
        let change_views = self
            .last_change_views
            .iter()
            .filter_map(|cv| cv.as_ref())
            .map(|cv| cv.to_change_view_compact())
            .collect();

        let preparations =
            self.prepares.iter().filter_map(|p| p.to_preparation_compact()).collect();

        let commits =
            self.commits.iter().filter_map(|c| c.as_ref()).map(|c| c.to_commit_compact()).collect();

        let req = self.prepares.iter().filter_map(|p| p.request.as_ref()).find(|_req| true);

        let prepare_stage = if let Some(req) = req {
            PrepareStage::Prepare(req.message.clone())
        } else {
            PrepareStage::Preparation(self.max_quorum_preparation())
        };

        Message {
            meta,
            message: RecoveryMessage { change_views, prepare_stage, preparations, commits },
        }
    }

    pub fn new_change_view(
        &self,
        meta: MessageMeta,
        unix_milli: u64,
        reason: ChangeViewReason,
    ) -> Message<ChangeViewRequest> {
        let new_view_number = meta.view_number + 1;
        Message { meta, message: ChangeViewRequest { new_view_number, unix_milli, reason } }
    }

    pub fn new_block_witness(&self, view_number: ViewNumber, validators: &[PublicKey]) -> Witness {
        let signers = byzantine_honest_quorum(validators.len() as u32);
        let contract =
            validators.to_multi_sign_contract(signers).expect("`validators` should be valid");

        let mut sign_cx = MultiSignContext::new(validators);
        for (idx, validator) in validators.iter().enumerate() {
            let Some(commit) = self.commits[idx].as_ref() else {
                continue;
            };
            if commit.meta.view_number != view_number {
                continue;
            }

            let _ = sign_cx.add_sign(validator, &commit.message.sign);
            if sign_cx.signs_count() >= signers as usize {
                break;
            }
        }

        let invocation = sign_cx.to_invocation_script();
        Witness::new(invocation, contract.script)
    }
}
