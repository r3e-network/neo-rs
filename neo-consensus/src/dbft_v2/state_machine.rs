// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::{byzantine_failure_quorum, byzantine_honest_quorum};
use crate::dbft_v2::{context::*, message::*, Settings};


const MAX_ADVANCED_BLOCKS: u64 = 8;


#[derive(Debug, Copy, Clone)]
pub enum Broadcasts {
    PrepareRequest,
    PrepareResponse,
    Commit,
    Block,
    RecoveryRequest,
}

pub struct StateMachine {
    pub(crate) states: ConsensusStates,
    pub(crate) context: ConsensusContext,
}


impl StateMachine {
    #[inline]
    pub fn states(&self) -> &ConsensusStates { &self.states }

    #[inline]
    pub fn context(&self) -> &ConsensusContext { &self.context }

    pub fn is_view_changing(&self) -> bool {
        let view_number = self.states.view_number;
        let index = self.states.self_index as usize;
        !self.states.watch_only &&
            index <= self.context.change_views.len() &&
            self.context.change_views[index].as_ref()
                .map(|(_, req)| req.new_view_number > view_number)
                .unwrap_or(false)
    }

    pub fn on_message(&mut self, settings: &Settings, message: Message) {
        match message {
            Message::ChangeView { meta, message } => self.on_change_view(settings, meta, message),
            Message::PrepareRequest { meta, message } => self.on_prepare_request(settings, meta, message),
            Message::PrepareResponse { meta, message } => self.on_prepare_response(meta, message),
            Message::Commit { meta, message } => self.on_commit(meta, message),
            Message::RecoveryRequest { meta, message } => self.on_recovery_request(meta, message),
            Message::RecoveryMessage { meta, message } => self.on_recovery_message(meta, message),
        }
    }
}


impl StateMachine {
    fn on_prepare_request(&self, settings: &Settings, meta: MessageMeta, message: PrepareRequest) {
        // 1.1 Ignore if the PrepareRequest has already been received
        let primary = self.states.primary_index as usize;
        if primary >= self.context.preparations.len() || self.context.preparations[primary].is_some() {
            return;
        }

        // 1.2 Ignore if the PrepareRequest if the node is trying to change the view
        let failures = byzantine_failure_quorum(settings.nr_validators) as usize;
        let block_index = self.states.block_index;
        if self.is_view_changing() &&
            self.context.commit_count() + self.context.failed_count(block_index, &[]) > failures {
            return;
        }

        // 2. Ignore if the payload.validator_index is not the index of the current round speaker or
        //   the payload.view_number is not equal to the current view number
        if meta.validator_index != self.states.primary_index || meta.view_number != self.states.view_number {
            return;
        }

        // 3. Ignore if message.version or message.prev_hash is different from the local context
        if message.version != settings.version || message.prev_hash != self.states.prev_hash {
            return;
        }

        // 4. Ignore if transactions amount is over MaxTransactionsPerBlock
        if message.tx_hashes.len() > settings.max_txs_per_block as usize {
            return;
        }

        // 5. Ignore if the message.timestamp is not more than the timestamp of the previous block,
        //   or is more than 8 blocks above current time
        let now = (settings.unix_milli_now)();
        if message.unix_milli <= self.states.prev_block_unix_milli ||
            message.unix_milli > now + (MAX_ADVANCED_BLOCKS * settings.millis_per_block()) {
            return;
        }

        // 6. Ignore if any proposed transaction has already been included in the blockchain

        // 7. Renew consensus context and clear invalid signatures that have been received,
        //   i.e. PrepareResponse may arrive first

        // 8. Save the signature of the speaker into current context

        // 9. If there's no transaction in this request, directly check the local collection of
        //  `PrepareResponse`, and broadcast the `Commit` message if enough `PrepareResponse` collected

        // 10.0. Collect and verify transactions in the proposal block from mem-pool(tx-pool)

        // 10.1. Ignore if the transaction failed to pass verification or
        //   the transaction did not meet strategic requirements

        // 10.2. Otherwise the transaction will be saved into current consensus context

        // 11. Verify the transactions required by blocks in the unconfirmed transaction pool
        //   and add them into current context

        // 12. Broadcast a `GetData` message with transaction hashes if they were missed in the block
    }
}


impl StateMachine {
    fn on_change_view(&mut self, settings: &Settings, meta: MessageMeta, message: ChangeViewRequest) {
        let new_view_number = message.new_view_number;

        // 1. Send RecoveryMessage if the new view number in the message is less than or equal
        //   to the view number in current context
        if new_view_number <= self.states.view_number { // on_recovery_request
            return;
        }

        // 2.1. Ignore it if the node has sent Commit
        if self.context.has_commit(self.states.self_index) { // ignore
            return;
        }

        // 2.2 Ignore if index is out of validators range
        let index = meta.validator_index as usize;
        if index >= self.context.change_views.len() { // ignore
            return;
        }

        // 3.1 If current node received at least N-f ChangeView messages with the same new view number,
        // then ViewChange will happen
        if let Some((_, prev)) = self.context.change_views[index].as_ref() {
            if new_view_number <= prev.new_view_number { // ignore
                return;
            }
        }

        // 3.2 The current node reset the consensus process with the new view number
        self.context.change_views[index] = Some((meta, message));
        self.change_view_if_satisfied(settings.nr_validators, new_view_number);
    }

    fn change_view_if_satisfied(&mut self, nr_validators: u32, view_number: ViewNumber) {
        if self.states.view_number >= view_number {
            return;
        }

        let count = self.context.change_views.iter()
            .filter(|change_view|
                change_view.as_ref()
                    .map(|(_, req)| req.new_view_number >= view_number)
                    .unwrap_or(false)
            ).count();

        if count >= byzantine_honest_quorum(nr_validators) as usize {
            if self.states.watch_only {
                //
            }

            // reset consensus
            self.context.reset(nr_validators);
        }
    }
}


impl StateMachine {
    fn on_prepare_response(&self, _meta: MessageMeta, _message: PrepareResponse) {
        //
    }

    fn on_commit(&self, _meta: MessageMeta, _message: Commit) {
        //
    }

    fn on_recovery_request(&self, _meta: MessageMeta, _message: RecoveryRequest) {
        //
    }

    fn on_recovery_message(&self, _meta: MessageMeta, _message: RecoveryMessage) {
        //
    }
}
