// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


pub mod committee;
pub mod context;

pub mod message;
pub mod state_machine;


use alloc::string::String;
use core::time::Duration;

use neo_base::{errors, encoding::bin::*};
use neo_core::{
    Keypair,
    block::{self, Header}, tx::Witness,
    payload::{Extensible, CONSENSUS_CATEGORY},
    types::{H160, ToScriptHash, ToCheckSign, ToBftHash},
};
use neo_crypto::rand;
use crate::{Block, dbft_v2::{committee::*, message::*, state_machine::*}};


#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct HView {
    /// The height of the chain, i.e. block number
    pub height: u32,

    /// The view number in DBFT v2.0
    pub view_number: ViewNumber,
}


#[derive(Debug)]
pub struct Settings {
    pub duration_per_block: Duration,
    pub network: u32,
    pub version: u32,

    pub nr_validators: u32,
    pub nr_committee: u32,

    pub max_txs_per_block: u32,
    pub max_block_size: usize,
    pub max_block_sysfee: usize,

    pub timestamp_increment: u64,
    pub unix_milli_now: fn() -> u64,

    pub recovery_logs: String,
    pub ignore_recovery_logs: bool,
}

impl Settings {
    pub(crate) fn next_block_timestamp(&self, now: u64, prev_block_unix_milli: u64) -> u64 {
        let ts = self.timestamp_increment + prev_block_unix_milli;
        let now = now / self.timestamp_increment * self.timestamp_increment;
        if now > ts { now } else { ts }
    }

    pub fn millis_per_block(&self) -> u64 { self.duration_per_block.as_millis() as u64 }
}


pub struct DbftConsensus<Members: MemberCache> {
    settings: Settings,
    committee: Committee<Members>,
    state_machine: StateMachine,
}


impl<Members: MemberCache> DbftConsensus<Members> {
    pub fn new_not_signed_block(&self) -> Block {
        let nonce = rand::read_u64()
            .expect("`rand_u64` should be ok");

        let validators = self.committee.compute_next_block_validators();
        let next_consensus = validators.to_bft_hash()
            .expect("`next_validators` should be valid");

        let states = self.state_machine.states();
        let context = self.state_machine.context();

        let now = (self.settings.unix_milli_now)();
        let witness = context.new_block_witness(states.view_number, &validators);
        let mut header = Header {
            hash: None,
            version: self.settings.version,
            prev_hash: states.prev_hash,
            merkle_root: context.merkle_root(),
            unix_milli: self.settings.next_block_timestamp(now, states.prev_block_unix_milli),
            nonce,
            index: states.block_index,
            primary: states.primary_index,
            next_consensus: next_consensus.into(),
            witnesses: witness.into(),
        };
        header.calc_hash();

        Block {
            network: self.settings.network,
            block: block::Block::new(header, context.txs()),
            sign: Default::default(),
        }
    }

    fn check_payload(&self, payload: &Extensible) -> Result<(), OnPayloadError> {
        // 1. Ignore the payload if ValidBlockStart is lower than ValidBlockEnd .
        if payload.valid_block_start >= payload.valid_block_end {
            return Err(OnPayloadError::InvalidPayload("valid_block_start >= valid_block_end"));
        }

        let states = self.state_machine.states();

        // 2. Ignore the payload if current block height is out of [ValidBlockStart, ValidBlockEnd) .
        if states.block_index < payload.valid_block_start || states.block_index >= payload.valid_block_end {
            return Err(OnPayloadError::InvalidPayload("current block index out of valid block range"));
        }

        // 3. Ignore the payload if sender is not listed in the consensus allow list

        // 4. Ignore the payload if the verification script failed or Category is not "dBFT"
        if !payload.category.eq(CONSENSUS_CATEGORY) {
            return Err(OnPayloadError::InvalidPayload("only 'dBFT' is supported at now"));
        }

        // 5. Ignore the message if the node has sent out the new block
        Ok(())
    }


    fn check_message(&self, sender: &H160, message: &Message) -> Result<(), OnPayloadError> {
        let states = self.state_machine.states();
        let meta = message.message_meta();

        // 7. Ignore the message if the message.BlockIndex is lower than the current block height
        if meta.block_index != states.block_index {
            return Err(OnPayloadError::InvalidMessageMeta("block_index", states.block_index, meta));
        }

        let validator_index = meta.validator_index as usize;
        let validators = self.committee.next_block_validators();

        // 8.1. Ignore the message if the `validator_index` is out of the current consensus nodes.
        if validator_index >= validators.len() {
            return Err(OnPayloadError::InvalidMessageMeta("validator_index", validators.len() as u32, meta));
        }

        // 8.2. Ignore the message if the payload.Sender is different from the correct hash
        let validator = validators[validator_index].to_script_hash().into();
        if !sender.eq(&validator) {
            return Err(OnPayloadError::InvalidSender(sender.clone(), meta.validator_index));
        }

        Ok(())
    }

    pub fn on_payload(&mut self, payload: &Extensible) -> Result<(), OnPayloadError> {
        let _ = self.check_payload(payload)?;

        // 6. Ignore the message if the consensus message data is in a wrong format
        let mut buffer = RefBuffer::from(payload.data.as_bytes());
        let message: Message = BinDecoder::decode_bin(&mut buffer)
            .map_err(|err| OnPayloadError::DecodeError(err))?;

        let _ = self.check_message(&payload.sender, &message)?;

        self.state_machine.on_message(&self.settings, message);
        Ok(())
    }
}


#[derive(Debug, errors::Error)]
pub enum OnPayloadError {
    #[error("on-payload: decode payload error: {0}")]
    DecodeError(BinDecodeError),

    #[error("on-payload: invalid payload error: '{0}'")]
    InvalidPayload(&'static str),

    #[error("on-payload: invalid message meta on '{0}' expected {1} but {2:?}")]
    InvalidMessageMeta(&'static str, u32, MessageMeta),

    #[error("on-payload: invalid sender '{0}' index {1}")]
    InvalidSender(H160, ViewIndex),
}

pub trait ToPayload {
    fn to_signed_payload(&self, network: u32, sender: &Keypair) -> Extensible;
}

impl ToPayload for Message {
    fn to_signed_payload(&self, network: u32, sender: &Keypair) -> Extensible {
        let meta = self.message_meta();
        let script = sender.public.to_check_sign();
        let mut ext = Extensible {
            category: CONSENSUS_CATEGORY.into(),
            valid_block_start: 0,
            valid_block_end: meta.block_index,
            sender: script.to_script_hash().into(),
            data: self.to_bin_encoded().into(),
            witnesses: Default::default(),
        };

        let invocation = ext.sign(network, sender.secret)
            .expect("`sign` payload should be ok")
            .to_invocation_script();

        ext.witnesses = Witness::new(invocation, script.into()).into();
        ext
    }
}
