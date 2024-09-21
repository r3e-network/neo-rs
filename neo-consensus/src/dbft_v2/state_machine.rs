// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::boxed::Box;
#[cfg(feature = "std")]
use std::collections::HashMap;

#[cfg(not(feature = "std"))]
use hashbrown::HashMap;
use neo_base::{byzantine_failure_quorum, byzantine_honest_quorum};
use neo_core::block::{self, Header};
use neo_core::merkle::MerkleSha256;
use neo_core::store::ChainStates;
use neo_core::types::{Sign, ToSignData, H256};
use neo_crypto::{
    ecdsa::{DigestVerify, Sign as EcdsaSign},
    rand,
};

use crate::dbft_v2::*;

const MAX_ADVANCED_BLOCKS: u64 = 8;

pub struct StateMachine {
    pub(crate) self_keypair: Keypair,
    pub(crate) timer: ViewTimer,
    pub(crate) broadcast_tx: mpsc::SyncSender<Payload>,

    pub(crate) unix_milli_now: fn() -> u64,
    pub(crate) chain: Box<dyn ChainStates>,
    pub(crate) committee: Committee,

    pub(crate) states: ConsensusStates,
    pub(crate) context: ConsensusContext,
    pub(crate) header: Option<Header>,
}

impl StateMachine {
    #[inline]
    pub fn states(&self) -> &ConsensusStates { &self.states }

    #[inline]
    pub fn context(&self) -> &ConsensusContext { &self.context }

    #[inline]
    pub fn unix_milli_now(&self) -> u64 { (self.unix_milli_now)() }

    pub fn is_view_changing(&self) -> bool {
        if self.states.watch_only {
            return false;
        }

        let view_number = self.states.view_number;
        let index = self.states.self_index as usize;
        index <= self.context.change_views.len()
            && self.context.change_views[index]
                .as_ref()
                .map(|cv| cv.message.new_view_number > view_number)
                .unwrap_or(false)
    }

    pub fn unacceptable_on_view_changing(&self) -> bool {
        if !self.is_view_changing() {
            return false;
        }

        let nr_validators = self.states.nr_validators() as u32;
        let quorum = byzantine_failure_quorum(nr_validators) as usize;

        let block_index = self.states.block_index;
        let fails = self.context.failed_count(block_index, &self.states.validators);
        self.context.commit_count() + fails <= quorum
    }

    pub(crate) fn make_header(&self, version: u32, view_number: ViewNumber) -> Option<Header> {
        let validators = self.states.validators.as_slice();
        let next_consensus = validators.to_bft_hash().expect("`next_validators` should be valid");

        let primary = self.states.primary_index as usize;
        let Some(r) = self.context.prepares[primary].request.as_ref() else {
            return None;
        };
        let mut head = Header {
            hash: None,
            version,
            prev_hash: self.states.prev_hash,
            merkle_root: r.message.tx_hashes.merkle_sha256(),
            unix_milli: r.message.unix_milli,
            nonce: r.message.nonce,
            index: self.states.block_index,
            primary: self.states.primary_index,
            next_consensus: next_consensus.into(),
            witnesses: self.context.new_block_witness(view_number, validators).into(),
        };

        head.calc_hash();
        Some(head)
    }

    pub(crate) fn try_make_header(
        &mut self,
        version: u32,
        view_number: ViewNumber,
    ) -> Option<&Header> {
        if self.header.is_some() {
            return self.header.as_ref();
        }

        self.header = self.make_header(version, view_number);
        self.header.as_ref()
    }

    pub(crate) fn reset_consensus(&mut self, view_number: ViewNumber, per_block_millis: u64) {
        if view_number == 0 {
            self.states.validators = self.committee.compute_next_block_validators();
        }

        let nr_validators = self.states.nr_validators() as u32;
        let new = ConsensusContext::new(nr_validators);
        let old = core::mem::replace(&mut self.context, new);
        if view_number == 0 {
            let current = self.chain.current_states();
            self.states.prev_hash = current.block_hash;
            self.states.block_index = current.block_index + 1;
        } else {
            for (idx, cv) in old.change_views.into_iter().enumerate() {
                if cv.as_ref().is_some_and(|v| v.message.new_view_number >= view_number) {
                    self.context.change_views[idx] = cv;
                } // else {  self.context.change_views[idx] = None; }
            }
        }

        let self_pk = &self.self_keypair.public;
        let block_index = self.states.block_index;

        self.states.view_number = view_number;
        self.states.self_index = 0;
        self.states.not_validator = true;
        self.states.primary_index = primary_index(block_index, view_number, nr_validators);
        for (idx, pk) in self.states.validators.iter().enumerate() {
            if pk.eq(self_pk) {
                self.states.self_index = idx as ViewIndex;
                self.states.not_validator = false;
                break;
            }
        }

        let view = HView { height: block_index, view_number };
        if !self.states.not_validator {
            self.context.last_seen_message.insert(self_pk.clone(), view);
        }

        self.header = None;
        self.states.block_sent = false;
        if self.states.watch_only {
            return;
        }

        let delay_millis = self.timeout_millis_on_resetting(per_block_millis);
        self.reset_timeout_millis(delay_millis);
    }

    fn timeout_millis_on_resetting(&self, per_block_millis: u64) -> u64 {
        let primary = self.states.is_primary() && !self.states.on_recovering;
        let millis = millis_on_resetting(primary, self.states.view_number, per_block_millis);

        let diff = if self.states.received_block_index + 1 == self.states.block_index {
            self.unix_milli_now() - self.states.received_unix_milli
        } else {
            0
        };

        if millis > diff { millis - diff } else { 0 }
    }

    fn reset_timeout_millis(&self, timeout_millis: u64) {
        self.timer.reset_timeout(self.states.height_view(), timeout_millis);
    }

    fn extend_timeout_millis(&self, max_in_blocks: u32, per_block_millis: u64) {
        let self_index = self.states.self_index;
        if !self.states.watch_only || self.is_view_changing() || self.context.has_commit(self_index)
        {
            let blocks_millis = per_block_millis * max_in_blocks as u64;
            let honest = byzantine_honest_quorum(self.states.nr_validators() as u32);

            let extend_millis = blocks_millis / honest as u64;
            self.timer.extend_timeout(self.states.height_view(), extend_millis);
        }
    }

    pub fn on_timeout(&mut self, settings: &DbftConfig, view: HView) {
        // TODO: log
        if self.states.watch_only
            || view.height != self.states.block_index
            || view.view_number != self.states.view_number
        {
            // TODO: block is sent
            return;
        }

        let per_block_millis = settings.per_block_millis;
        let primary = self.states.primary_index as usize;
        let sent =
            primary < self.context.prepares.len() && self.context.prepares[primary].has_request();

        if self.states.is_primary() && !sent {
            self.context.txs = HashMap::new(); // TODO: from tx-pool
            self.context.tx_hashes =
                self.context.txs.iter().map(|(hash, _)| hash.clone()).collect();

            let meta = self.states.new_message_meta();
            let message =
                self.new_prepare_request(settings.version, settings.milli_increment, meta);
            self.broadcast_tx
                .send(Payload::PrepareRequest(message))
                .expect("`broadcast_tx.send(PrepareRequest)` should be ok");

            if self.states.nr_validators() == 1 {
                self.commit_if_needed(settings.network, per_block_millis);
            }

            if self.context.tx_hashes.len() > 0 {
                // TODO: send txs
            }

            self.reset_timeout_millis(millis_on_timeout(self.states.view_number, per_block_millis));
            return;
        }

        if (self.states.is_primary() && sent) || self.states.is_backup() {
            if self.context.has_commit(self.states.self_index) {
                let meta = self.states.new_message_meta();
                let recovery = self.context.new_recovery_message(meta);
                self.broadcast_tx
                    .send(Payload::RecoveryMessage(recovery))
                    .expect("`broadcast_tx.send(RecoveryMessage)` should be ok");
            } else {
                let reason = if self.context.has_all_txs() {
                    ChangeViewReason::Timeout
                } else {
                    ChangeViewReason::TxNotFound
                };

                self.try_to_change_view(reason, per_block_millis);
            }
        }
    }

    pub fn on_message(&mut self, settings: &DbftConfig, message: Payload) {
        let meta = message.message_meta();
        let validator = meta.validator_index as usize;
        if validator < self.states.validators.len() {
            let current = meta.height_view();
            let validator = &self.states.validators[validator];
            let existed = self.context.last_seen_message.get(validator);
            if existed.is_some_and(|v| v.is_previous(&current)) || existed.is_none() {
                self.context.last_seen_message.insert(validator.clone(), current);
            }
        }

        match message {
            Payload::ChangeView(m) => self.on_change_view(settings, m),
            Payload::PrepareRequest(m) => self.on_prepare_request(settings, m),
            Payload::PrepareResponse(m) => self.on_prepare_response(settings, m),
            Payload::Commit(m) => self.on_commit(settings, m),
            Payload::RecoveryRequest(m) => self.on_recovery_request(m),
            Payload::RecoveryMessage(m) => self.on_recovery_message(settings, m),
        }
    }
}

// StateMachine action on PrepareRequest
impl StateMachine {
    fn new_prepare_request(
        &self,
        version: u32,
        milli_increment: u64,
        meta: MessageMeta,
    ) -> Message<PrepareRequest> {
        let nonce = rand::read_u64().expect("`rand_u64` should be ok");
        let unix_milli = next_block_unix_milli(
            self.unix_milli_now(),
            milli_increment,
            self.states.received_unix_milli,
        );

        Message {
            meta,
            message: PrepareRequest {
                version,
                prev_hash: self.states.prev_hash,
                unix_milli,
                nonce,
                tx_hashes: self.context.tx_hashes.clone(),
                payload_hash: H256::default(), // just ignore
            },
        }
    }

    fn check_prepare_request(
        &self,
        settings: &DbftConfig,
        prepare: &Message<PrepareRequest>,
    ) -> bool {
        let meta = prepare.meta;
        let message = &prepare.message;

        // 1.1 Ignore if the PrepareRequest has already been received
        let primary = self.states.primary_index as usize;
        if primary >= self.context.prepares.len() || self.context.prepares[primary].has_request() {
            return false;
        }

        // 1.2 Ignore if the PrepareRequest if the node is trying to change the view
        if self.unacceptable_on_view_changing() {
            return false;
        }

        // 2. Ignore if the payload.validator_index is not the index of the current round speaker or
        //   the payload.view_number is not equal to the current view number
        if meta.validator_index != self.states.primary_index
            || meta.view_number != self.states.view_number
        {
            return false;
        }

        // 3. Ignore if message.version or message.prev_hash is different from the local context
        if message.version != settings.version || message.prev_hash != self.states.prev_hash {
            return false;
        }

        // 4. Ignore if transactions amount is over MaxTransactionsPerBlock
        if message.tx_hashes.len() > settings.max_txs_per_block as usize {
            return false;
        }

        // 5. Ignore if the message.timestamp is not more than the timestamp of the previous block,
        //   or is more than 8 blocks above current time
        let now = self.unix_milli_now();
        !(message.unix_milli <= self.states.received_unix_milli
            || message.unix_milli > now + (MAX_ADVANCED_BLOCKS * settings.per_block_millis))
    }

    fn on_prepare_request(&mut self, settings: &DbftConfig, prepare: Message<PrepareRequest>) {
        if !self.check_prepare_request(settings, &prepare) {
            return;
        }

        // 6.0 Ignore if any proposed transaction has already been included in the blockchain
        let message = &prepare.message;
        if message.tx_hashes.iter().any(|tx| self.chain.contains_tx(tx)) {
            return;
        }

        //  6.1 extend timer by factor 2
        self.extend_timeout_millis(2, settings.per_block_millis);

        // 7. Renew consensus context and clear invalid signatures that have been received,
        //   i.e. PrepareResponse may arrive first
        let view_number = prepare.meta.view_number;
        let tx_hashes_empty = message.tx_hashes.is_empty();
        let payload_hash = self.context.on_prepare_received(prepare);

        self.header = self.make_header(settings.version, view_number);
        let sign_data = self
            .header
            .as_ref()
            .map(|head| head.to_sign_data(settings.network))
            .expect("`to_sign_data` should be ok"); // PrepareRequest saved in on_prepare_received

        // 8. Save the signature of the speaker into current context
        for commit in self.context.commits.iter_mut() {
            let Some(m) = commit.as_ref() else {
                continue;
            };
            let validator = m.meta.validator_index as usize;
            if m.meta.view_number != self.states.view_number
                || validator >= self.states.validators.len()
            {
                continue;
            }

            let pk = &self.states.validators[validator];
            if pk.verify_digest(&sign_data, m.message.sign.as_bytes()).is_err() {
                *commit = None;
            }
        }

        // 9. If there's no transaction in this request, directly check the local collection of
        //  `PrepareResponse`, and then broadcast the `Commit` message if enough `PrepareResponse` collected
        if tx_hashes_empty {
            self.response_prepare_if_needed(settings, payload_hash);
            return;
        }

        // 10.0. Collect and verify transactions in the proposal block from mem-pool(tx-pool)

        // 10.1. Ignore if the transaction failed to pass verification or
        //   the transaction did not meet strategic requirements

        // 10.2. Otherwise the transaction will be saved into current consensus context

        // 11. Verify the transactions required by blocks in the unconfirmed transaction pool
        //   and add them into current context

        // 12. Broadcast a `GetData` message with transaction hashes if they were missed in the block
    }

    fn response_prepare_if_needed(&mut self, settings: &DbftConfig, payload_hash: H256) {
        if !self.context.has_all_txs() {
            return;
        }

        if self.states.is_primary() || self.states.watch_only {
            return;
        }

        let head_size = self.header.as_ref().map(|h| h.bin_size()).unwrap_or(0);
        let (count_size, _) = to_varint_le(self.context.tx_hashes.len() as u64);

        let per_block_millis = settings.per_block_millis;
        let (txs_size, sysfee) = self
            .context
            .txs
            .iter()
            .map(|(_, tx)| (tx.bin_size(), tx.sysfee))
            .fold((0usize, 0u64), |acc, x| (acc.0 + x.0, acc.1 + x.1));

        if sysfee > settings.max_block_sysfee
            || head_size + (count_size as usize) + txs_size > settings.max_block_size
        {
            self.try_to_change_view(ChangeViewReason::BlockRejectedByPolicy, per_block_millis);
            return;
        }

        self.extend_timeout_millis(2, per_block_millis);

        let message = Message {
            meta: self.states.new_message_meta(),
            message: PrepareResponse { preparation: payload_hash },
        };
        self.broadcast_tx
            .send(Payload::PrepareResponse(message))
            .expect("`broadcast_tx.send(PrepareResponse)` should be ok");

        self.commit_if_needed(settings.network, per_block_millis);
    }

    fn try_to_change_view(&mut self, reason: ChangeViewReason, per_block_millis: u64) {
        if self.states.watch_only {
            return;
        }

        let changed_view = self.states.view_number + 1;
        self.reset_timeout_millis(millis_on_setting(changed_view, per_block_millis));

        let nr_validators = self.states.nr_validators() as u32;
        let quorum = byzantine_failure_quorum(nr_validators) as usize;

        let fails = self.context.failed_count(self.states.block_index, &self.states.validators);
        if self.context.commit_count() + fails > quorum {
            // TODO: log
            let message = Message {
                meta: self.states.new_message_meta(),
                message: RecoveryRequest {
                    unix_milli: self.unix_milli_now(),
                    payload_hash: H256::default(),
                },
            };
            self.broadcast_tx
                .send(Payload::RecoveryRequest(message))
                .expect("`broadcast_tx.send(RecoveryRequest)` should be ok");
        } else {
            // TODO: log
            let change_view = self.context.new_change_view(
                self.states.new_message_meta(),
                self.unix_milli_now(),
                reason,
            );

            let myself = self.states.self_index as usize;
            self.context.change_views[myself] = Some(change_view.clone());
            self.broadcast_tx
                .send(Payload::ChangeView(change_view))
                .expect("`broadcast_tx.send(ChangeView)` should be ok");

            self.change_view_if_needed(per_block_millis, changed_view);
        }
    }
}

impl StateMachine {
    fn check_prepare_response(&self, prepare: &Message<PrepareResponse>) -> bool {
        // 1. Ignore it if the message view is not the same as the current view
        if prepare.meta.view_number != self.states.view_number {
            return false;
        }

        // 2.1 Ignore it if current node has already saved the sender's signature
        let validator = prepare.meta.validator_index as usize;
        if validator >= self.context.prepares.len()
            || self.context.prepares[validator].has_response()
        {
            return false;
        }

        // 2.2 or he current node is trying to change the view
        if self.unacceptable_on_view_changing() {
            return false;
        }

        // 3. Save it temporarily if current node has not received PrepareResponse yet
        //  (Clear it after receiving PrepareResponse), or go to next step.
        let message = &prepare.message;
        let primary = self.states.primary_index as usize;
        primary < self.context.prepares.len()
            && self.context.prepares[primary]
                .response
                .as_ref()
                .map(|r| r.message.preparation == message.preparation)
                .unwrap_or(true)
    }

    fn on_prepare_response(&mut self, settings: &DbftConfig, prepare: Message<PrepareResponse>) {
        if !self.check_prepare_response(&prepare) {
            return;
        }

        // extend timer by factor 2;
        self.extend_timeout_millis(2, settings.per_block_millis);

        // 4. Verify the signature. Save the signature if it pass the verification. Ignore it if not.
        let validator = prepare.meta.validator_index as usize; // out-of-bound has checked
        self.context.prepares[validator].response = Some(prepare);

        // 5. Ignore it if the node has already sent Commit.
        if self.states.watch_only || self.context.has_commit(self.states.self_index) {
            return;
        }

        // 6. Verify the signature number if the node has already sent or received PrepareRequest.
        //  If there are at least N-f signatures, broadcast Commit and generate the block if there
        //  are N-f Commit messages have been received.
        let prepares = &self.context.prepares[self.states.primary_index as usize];
        if prepares.has_request() || prepares.has_response() {
            self.commit_if_needed(settings.network, settings.per_block_millis);
        }
    }

    fn commit_if_needed(&mut self, network: u32, per_block_millis: u64) {
        let prepares =
            self.context.prepares.iter().filter(|p| p.has_request() || p.has_response()).count();

        let honest = byzantine_honest_quorum(self.states.nr_validators() as u32);
        if prepares < honest as usize || !self.context.has_all_txs() {
            return;
        }

        let Some(head) = self.header.as_ref() else {
            return; // TODO: log
        };
        let sign_data = head.to_sign_data(network);
        let sign = self.self_keypair.secret.sign(&sign_data).expect("`sign(header)` should be ok");

        let commit = Message {
            meta: self.states.new_message_meta(),
            message: Commit { sign: Sign::from(sign) },
        };
        self.broadcast_tx
            .send(Payload::Commit(commit))
            .expect("`broadcast_tx.send(Commit)` should be ok");

        self.reset_timeout_millis(millis_on_setting(self.states.view_number, per_block_millis));
        self.create_block_if_needed();
    }
}

impl StateMachine {
    fn create_block_if_needed(&mut self) {
        let commits = self
            .context
            .commits
            .iter()
            .filter(|commit| {
                commit.as_ref().is_some_and(|m| m.meta.view_number == self.states.view_number)
            })
            .count();

        let honest = byzantine_honest_quorum(self.states.nr_validators() as u32);
        if commits < honest as usize || !self.context.has_all_txs() {
            return;
        }

        let Some(head) = self.header.as_ref() else {
            return;
        };
        self.states.received_block_index = head.index;
        self.states.received_unix_milli = self.unix_milli_now();

        let _block = block::Block::new(head.clone(), self.context.txs());
        // TODO: send block
    }

    fn on_commit(&mut self, settings: &DbftConfig, commit: Message<Commit>) {
        // 0. On receiving a Commit send by consensus nodes after receiving N-f PrepareResponse.

        // 1. Ignore it if it has been received from the same node before.
        if self.context.has_commit(commit.meta.validator_index) {
            return; // TODO: log
        }

        // Receiving commit from another view
        let view_number = commit.meta.view_number;
        let validator = commit.meta.validator_index as usize; // out-of-bound has checked
        if view_number != self.states.view_number {
            self.context.commits[validator] = Some(commit);
            return;
        }

        // extend timer by factor 4
        self.extend_timeout_millis(4, settings.per_block_millis);

        // 2. Save the message into the consensus context if the signature passed verification,
        //   generate a block and broadcast if N-f Commit messages has been received.
        let Some(head) = self.try_make_header(settings.version, view_number) else {
            return;
        };
        let sign_data = head.to_sign_data(settings.network);
        let pk = &self.states.validators[validator];
        if pk.verify_digest(&sign_data, commit.message.sign.as_bytes()).is_err() {
            return; // TODO: log
        }

        self.create_block_if_needed();
    }
}

// StateMachine action on Message<ChangeViewRequest>
impl StateMachine {
    fn check_change_view(&self, chang_view: &Message<ChangeViewRequest>) -> bool {
        let meta = chang_view.meta;
        let message = &chang_view.message;
        let new_view_number = message.new_view_number;

        // 1. Send RecoveryMessage if the new view number in the message is less than or equal
        //   to the view number in current context
        if new_view_number <= self.states.view_number {
            let message =
                RecoveryRequest { unix_milli: message.unix_milli, payload_hash: H256::default() };
            self.on_recovery_request(Message { meta, message });
            return false;
        }

        // 2.1. Ignore it if the node has sent Commit
        if self.context.has_commit(self.states.self_index) {
            let meta = self.states.new_message_meta();
            let recovery = self.context.new_recovery_message(meta);
            self.broadcast_tx
                .send(Payload::RecoveryMessage(recovery))
                .expect("`broadcast_tx.send(RecoveryMessage)` should be ok");
            return false;
        }

        // 2.2 Ignore if index is out of validators range
        let validator = meta.validator_index as usize;
        if validator >= self.context.change_views.len() {
            return false;
        }

        // 2.3 Ignore if has received a ChangeViewRequest message with larger new_view_number
        let existed_view = self.context.change_views[validator]
            .as_ref()
            .map(|cv| cv.message.new_view_number)
            .unwrap_or(0);

        new_view_number > existed_view
    }

    fn on_change_view(&mut self, settings: &DbftConfig, chang_view: Message<ChangeViewRequest>) {
        if !self.check_change_view(&chang_view) {
            return;
        }

        let validator = chang_view.meta.validator_index as usize;
        if validator >= self.context.change_views.len() {
            // ignore if out of bound
            return;
        }

        let new_view_number = chang_view.message.new_view_number;
        self.context.change_views[validator] = Some(chang_view);
        self.change_view_if_needed(settings.per_block_millis, new_view_number);
    }

    fn change_view_if_needed(&mut self, per_block_millis: u64, new_view_number: ViewNumber) {
        if self.states.view_number >= new_view_number {
            return;
        }

        // 3. If current node received at least N-f ChangeView messages with the same new view number,
        //   then ViewChange will happen
        let newer_views = self
            .context
            .change_views
            .iter()
            .filter(|change_view| {
                change_view.as_ref().is_some_and(|cv| cv.message.new_view_number >= new_view_number)
            })
            .count();

        if newer_views < byzantine_honest_quorum(self.states.nr_validators() as u32) as usize {
            return;
        }

        if !self.states.watch_only {
            let myself = self.states.self_index as usize;
            let should_send = self.context.change_views[myself]
                .as_ref()
                .map(|cv| cv.message.new_view_number < new_view_number)
                .unwrap_or(true); // change_views[myself] is None || change_views[myself].new_view_number < new_view_number
            if should_send {
                let change_view = self.context.new_change_view(
                    self.states.new_message_meta(),
                    self.unix_milli_now(),
                    ChangeViewReason::ChangeAgreement,
                );
                self.context.change_views[myself] = Some(change_view.clone());
                self.broadcast_tx
                    .send(Payload::ChangeView(change_view))
                    .expect("`broadcast_tx.send(ChangeView)` should be ok");
            }
        }

        // 4. The current node reset the consensus process with the new view number
        self.reset_consensus(new_view_number, per_block_millis);
    }
}

impl StateMachine {
    // 0. On receiving a RecoveryRequest sent by consensus nodes when initiating a consensus
    //  or the sum of committed and failed nodes is greater than 'f'
    fn on_recovery_request(&self, recovery: Message<RecoveryRequest>) {
        // 1.1 Ignore it if it has been received before.
        let _payload_hash = &recovery.message.payload_hash; // TODO: it has been received or not

        // 1.2 Ignore if local-node is watch only
        if self.states.watch_only {
            return;
        }

        // 2. Response it if the node has sent the Commit message before or the node index
        //  is no more than f numbers later than the sender index
        let validator = recovery.meta.validator_index as u32;
        let self_index = self.states.self_index;
        if !self.context.has_commit(self_index) {
            let nr_validators = self.states.nr_validators() as u32;
            let recoveries = byzantine_failure_quorum(nr_validators) + 1;
            let should_send = (1..=recoveries)
                .into_iter()
                .find(|idx| (validator + idx) % nr_validators == self_index as u32)
                .is_some();
            if !should_send {
                return; // TODO: log
            }
        }

        // 3. Send RecoveryMessage if the node is obligated to response
        let meta = self.states.new_message_meta();
        let recovery = self.context.new_recovery_message(meta);
        self.broadcast_tx
            .send(Payload::RecoveryMessage(recovery))
            .expect("`broadcast_tx.send(RecoveryMessage)` should be ok");
    }
}

impl StateMachine {
    // 0. On receiving a RecoveryMessage broadcast by consensus nodes when receiving
    //  an accessible RecoveryRequest or time out after a Commit message has been sent.
    #[inline]
    fn on_recovery_message(&mut self, settings: &DbftConfig, recovery: Message<RecoveryMessage>) {
        self.states.on_recovering = true;
        self.on_recovery_message_inner(settings, recovery);
        self.states.on_recovering = false;
    }

    fn on_recovery_message_inner(
        &mut self,
        settings: &DbftConfig,
        recovery: Message<RecoveryMessage>,
    ) {
        let meta = recovery.meta;
        let message = &recovery.message;

        // 1. Receive and handle ChangeView inside if the message view number is greater than the node view number.
        if meta.view_number > self.states.view_number {
            if self.context.has_commit(self.states.self_index) {
                return;
            }

            let change_views = message.change_views(meta.block_index);
            // let nr_change_views = change_views.len();
            for cv in change_views {
                self.on_message(settings, Payload::ChangeView(cv));
            }
        }

        if meta.view_number == self.states.view_number
            && !self.context.has_commit(self.states.self_index)
            && !self.unacceptable_on_view_changing()
        {
            if !self.context.has_preparation(self.states.primary_index) {
                let meta = MessageMeta {
                    block_index: meta.block_index,
                    validator_index: self.states.primary_index,
                    view_number: meta.view_number,
                };
                if let Some(r) = message.prepare_request(meta) {
                    self.on_message(settings, Payload::PrepareRequest(r));
                }
            }

            let responses = message.prepare_responses(meta.block_index, meta.view_number);
            // let nr_responses = responses.len();
            for r in responses {
                self.on_message(settings, Payload::PrepareResponse(r));
            }
        }

        if meta.view_number <= self.states.view_number {
            let commits = message.commits(meta.block_index);
            // let nr_commits = commits.len();
            for commit in commits {
                self.on_message(settings, Payload::Commit(commit));
            }
        }
    }
}
