// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;
use core::fmt::Debug;

use crate::dbft_v2::*;
use neo_base::encoding::bin::*;
use neo_core::types::{Sign, UInt256};

pub type ViewNumber = u8;

pub type ViewIndex = u8;

#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum MessageType {
    ChangeView = 0x00,
    PrepareRequest = 0x20,
    PrepareResponse = 0x21,
    Commit = 0x30,
    RecoveryRequest = 0x40,
    RecoveryMessage = 0x41,
}

#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
pub struct MessageMeta {
    pub block_index: u32,
    pub validator_index: ViewIndex,
    pub view_number: ViewNumber,
}

impl MessageMeta {
    #[inline]
    pub fn height_view(&self) -> HView {
        HView {
            height: self.block_index,
            view_number: self.view_number,
        }
    }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct Message<M: Debug + Clone + BinEncoder + BinDecoder> {
    pub meta: MessageMeta,
    pub message: M,
}

impl Message<ChangeViewRequest> {
    pub fn to_change_view_compact(&self) -> ChangeViewCompact {
        ChangeViewCompact {
            validator_index: self.meta.validator_index,
            original_view_number: self.meta.view_number,
            unix_milli: self.message.unix_milli,
            invocation_script: Bytes::default(), // TODO: sign
        }
    }
}

impl Message<Commit> {
    pub fn to_commit_compact(&self) -> CommitCompact {
        CommitCompact {
            view_number: self.meta.view_number,
            validator_index: self.meta.validator_index,
            sign: self.message.sign.clone(),
            invocation_script: Bytes::default(), // TODO: sign
        }
    }
}

impl Message<PrepareRequest> {
    pub fn to_preparation_compact(&self) -> PreparationCompact {
        PreparationCompact {
            validator_index: self.meta.validator_index,
            invocation_script: Bytes::default(), // TODO: sign
        }
    }
}

impl Message<PrepareResponse> {
    pub fn to_preparation_compact(&self) -> PreparationCompact {
        PreparationCompact {
            validator_index: self.meta.validator_index,
            invocation_script: Bytes::default(), // TODO: sign
        }
    }
}

#[derive(Debug, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum Payload {
    #[bin(tag = 0x00)]
    ChangeView(Message<ChangeViewRequest>),

    #[bin(tag = 0x20)]
    PrepareRequest(Message<PrepareRequest>),

    #[bin(tag = 0x21)]
    PrepareResponse(Message<PrepareResponse>),

    #[bin(tag = 0x30)]
    Commit(Message<Commit>),

    #[bin(tag = 0x40)]
    RecoveryRequest(Message<RecoveryRequest>),

    #[bin(tag = 0x41)]
    RecoveryMessage(Message<RecoveryMessage>),
}

impl Payload {
    pub fn message_type(&self) -> MessageType {
        use MessageType::*;
        match self {
            Self::ChangeView { .. } => ChangeView,
            Self::PrepareRequest { .. } => PrepareRequest,
            Self::PrepareResponse { .. } => PrepareResponse,
            Self::Commit { .. } => Commit,
            Self::RecoveryRequest { .. } => RecoveryRequest,
            Self::RecoveryMessage { .. } => RecoveryMessage,
        }
    }

    pub fn message_meta(&self) -> MessageMeta {
        match self {
            Self::ChangeView(m) => m.meta,
            Self::PrepareRequest(m) => m.meta,
            Self::PrepareResponse(m) => m.meta,
            Self::Commit(m) => m.meta,
            Self::RecoveryRequest(m) => m.meta,
            Self::RecoveryMessage(m) => m.meta,
        }
    }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct PrepareRequest {
    pub version: u32,
    pub prev_hash: UInt256,
    pub unix_milli: u64,
    pub nonce: u64,
    pub tx_hashes: Vec<UInt256>,

    /// Extensible hash that contains this PrepareRequest
    #[bin(ignore)]
    pub payload_hash: UInt256,
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct PrepareResponse {
    pub preparation: UInt256,
}

impl PrepareResponse {
    #[inline]
    pub fn new(prepare_request_hash: UInt256) -> Self {
        Self {
            preparation: prepare_request_hash,
        }
    }

    #[inline]
    pub fn new_payload(meta: MessageMeta, prepare_request_hash: UInt256) -> Payload {
        Payload::PrepareResponse(Message {
            meta,
            message: Self::new(prepare_request_hash),
        })
    }
}

#[derive(Debug, Copy, Clone, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum ChangeViewReason {
    Timeout = 0x00,
    ChangeAgreement = 0x01,
    TxNotFound = 0x02,
    TxRejectedByPolicy = 0x03,
    TxInvalid = 0x04,
    BlockRejectedByPolicy = 0x05,
    Unknown = 0xff,
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct Commit {
    pub sign: Sign,
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct ChangeViewRequest {
    #[bin(ignore)]
    pub new_view_number: ViewNumber,
    pub unix_milli: u64,
    pub reason: ChangeViewReason,
}
