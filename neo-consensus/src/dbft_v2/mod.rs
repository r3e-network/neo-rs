// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub mod committee;
pub mod context;
pub mod message;
pub mod recovery;
pub mod state_machine;
pub mod timer;

#[cfg(test)]
mod state_machine_test;

use alloc::string::String;
use std::sync::mpsc;

use neo_base::{encoding::bin::*, errors, time::unix_millis_now};
use neo_core::payload::{Extensible, CONSENSUS_CATEGORY};
use neo_core::store::ChainStates;
use neo_core::{tx, types::*, Keypair};

pub use {committee::*, context::*, message::*, recovery::*, state_machine::*, timer::*};

#[derive(Debug, Copy, Clone)]
pub enum Broadcasts {
    PrepareRequest,
    PrepareResponse,
    Commit,
    Block,
    RecoveryRequest,
}

#[derive(Debug, Copy, Clone, Default, Hash, Eq, PartialEq)]
pub struct HView {
    /// The height of the chain, i.e. block number
    pub height: u32,

    /// The view number in DBFT v2.0
    pub view_number: ViewNumber,
}

impl HView {
    #[inline]
    pub fn zero(&self) -> bool {
        self.eq(&Self::default())
    }

    #[inline]
    pub fn is_previous(&self, other: &HView) -> bool {
        self.height < other.height
            || (self.height == other.height && self.view_number < other.view_number)
    }
}

#[derive(Debug)]
pub struct DbftConfig {
    pub network: u32,
    pub version: u32,
    pub per_block_millis: u64,

    pub max_txs_per_block: u32,
    pub max_block_size: usize,
    pub max_block_sysfee: u64,

    pub max_pending_broadcasts: u32,

    /// i.e. timestamp_increment
    pub milli_increment: u64,
    pub recovery_logs: String,
    pub ignore_recovery_logs: bool,
}

impl Default for DbftConfig {
    fn default() -> Self {
        Self {
            network: Network::DevNet.as_magic(),
            version: 0,
            per_block_millis: DEFAULT_PER_BLOCK_MILLIS,
            max_txs_per_block: DEFAULT_MAX_TXS_PER_BLOCK,
            max_block_size: DEFAULT_MAX_BLOCK_SIZE,
            max_block_sysfee: DEFAULT_MAX_BLOCK_SYSFEE,
            max_pending_broadcasts: DEFAULT_MAX_PENDING_BROADCASTS,
            milli_increment: max_block_timestamp_increment(DEFAULT_PER_BLOCK_MILLIS),
            recovery_logs: "".into(),
            ignore_recovery_logs: true,
        }
    }
}

#[inline]
pub fn next_block_unix_milli(now: u64, milli_increment: u64, prev_block_unix_milli: u64) -> u64 {
    let timestamp = milli_increment + prev_block_unix_milli;
    let now = now / milli_increment * milli_increment;
    core::cmp::max(now, timestamp)
}

pub struct DbftConsensus {
    settings: DbftConfig,
    state_machine: StateMachine,
    timer_rx: mpsc::Receiver<Timer>,
    broadcast_rx: mpsc::Receiver<Payload>,
}

impl DbftConsensus {
    pub fn new(
        settings: DbftConfig,
        self_keypair: Keypair,
        committee: Committee,
        chain: Box<dyn ChainStates>,
    ) -> Self {
        let nr_validators = committee.nr_validators;
        let per_block_millis = settings.per_block_millis;
        let max_pending = settings.max_pending_broadcasts as usize;

        let (timer_tx, timer_rx) = mpsc::sync_channel(1);
        let (broadcast_tx, broadcast_rx) = mpsc::sync_channel(max_pending);

        let mut dbft = Self {
            settings,
            state_machine: StateMachine {
                self_keypair,
                timer: ViewTimer::new(unix_millis_now, timer_tx),
                broadcast_tx,
                unix_milli_now: unix_millis_now,
                chain,
                committee,
                states: ConsensusStates::new(),
                context: ConsensusContext::new(nr_validators),
                header: None,
            },
            timer_rx,
            broadcast_rx,
        };

        dbft.state_machine.reset_consensus(0, per_block_millis);
        dbft
    }

    fn check_payload(&self, payload: &Extensible) -> Result<(), OnPayloadError> {
        // 1. Ignore the payload if ValidBlockStart is lower than ValidBlockEnd.
        if payload.valid_block_start >= payload.valid_block_end {
            return Err(OnPayloadError::InvalidPayload(
                "valid_block_start >= valid_block_end",
            ));
        }

        // 2. Ignore the payload if current block height is out of [ValidBlockStart, ValidBlockEnd).
        let states = self.state_machine.states();
        if states.block_index < payload.valid_block_start
            || states.block_index >= payload.valid_block_end
        {
            return Err(OnPayloadError::InvalidPayload(
                "current block index out of valid block range",
            ));
        }

        // 3. Ignore the payload if sender is not listed in the consensus allow list

        // 4. Ignore the payload if the verification script failed or Category is not "dBFT"
        if !payload.category.eq(CONSENSUS_CATEGORY) {
            return Err(OnPayloadError::InvalidPayload(
                "only 'dBFT' is supported at now",
            ));
        }

        // 5. Ignore the message if the node has sent out the new block
        Ok(())
    }

    fn check_message(&self, sender: &UInt160, meta: MessageMeta) -> Result<(), OnPayloadError> {
        // 7. Ignore the message if the message.BlockIndex is lower than the current block height
        let states = self.state_machine.states();
        if meta.block_index != states.block_index {
            // TODO: add to cached message if  meta.block_index > states.block_index
            return Err(OnPayloadError::InvalidMessageMeta(
                "block_index",
                states.block_index,
                meta,
            ));
        }

        let index = meta.validator_index as usize;
        let validators = &states.validators;

        // 8.1. Ignore the message if the `validator_index` is out of the current consensus nodes.
        if index >= validators.len() {
            return Err(OnPayloadError::InvalidMessageMeta(
                "validator_index",
                validators.len() as u32,
                meta,
            ));
        }

        // 8.2. Ignore the message if the payload.Sender is different from the correct hash
        let validator = validators[index].to_script_hash().into();
        if !sender.eq(&validator) {
            return Err(OnPayloadError::InvalidSender(
                sender.clone(),
                meta.validator_index,
            ));
        }
        Ok(())
    }

    pub fn on_payload(&mut self, payload: &Extensible) -> Result<(), OnPayloadError> {
        let _ = self.check_payload(payload)?;

        // 6. Ignore the message if the consensus message data is in a wrong format
        let mut buffer = RefBuffer::from(payload.data.as_bytes());
        let mut message: Payload =
            BinDecoder::decode_bin(&mut buffer).map_err(|err| OnPayloadError::DecodeError(err))?;

        match &mut message {
            Payload::PrepareRequest(r) => {
                r.message.payload_hash = payload.hash_fields_sha256().into()
            }
            Payload::RecoveryRequest(r) => {
                r.message.payload_hash = payload.hash_fields_sha256().into()
            }
            _ => {}
        }

        let _ = self.check_message(&payload.sender, message.message_meta())?;
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
    InvalidSender(UInt160, ViewIndex),
}

pub trait ToPayload {
    fn to_signed_payload(&self, network: u32, sender: &Keypair) -> Extensible;
}

impl ToPayload for Payload {
    fn to_signed_payload(&self, network: u32, sender: &Keypair) -> Extensible {
        let meta = self.message_meta();
        let script = sender.public.to_check_sign();
        let mut ext = Extensible {
            category: CONSENSUS_CATEGORY.into(),
            valid_block_start: 0,
            valid_block_end: meta.block_index,
            sender: script.to_script_hash().into(),
            data: self.to_bin_encoded().into(),
            witnesses: tx::Witnesses::default(),
        };

        let invocation = ext
            .sign(network, &sender.secret)
            .expect("`sign` payload should be ok")
            .to_invocation_script();

        ext.witnesses = tx::Witness::new(invocation, script.into()).into();
        ext
    }
}
