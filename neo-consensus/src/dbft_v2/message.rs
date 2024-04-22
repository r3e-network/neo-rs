// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::vec::Vec;
use neo_base::encoding::bin::*;

use neo_core::types::{Bytes, H256, H256_SIZE, Sign};


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

    // #[bin(ignore)]
    // pub network: u32,
}


#[derive(Debug, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum Message {
    #[bin(tag = 0x00)]
    ChangeView { meta: MessageMeta, message: ChangeViewRequest },

    #[bin(tag = 0x20)]
    PrepareRequest { meta: MessageMeta, message: PrepareRequest },

    #[bin(tag = 0x21)]
    PrepareResponse { meta: MessageMeta, message: PrepareResponse },

    #[bin(tag = 0x30)]
    Commit { meta: MessageMeta, message: Commit },

    #[bin(tag = 0x40)]
    RecoveryRequest { meta: MessageMeta, message: RecoveryRequest },

    #[bin(tag = 0x41)]
    RecoveryMessage { meta: MessageMeta, message: RecoveryMessage },
}


impl Message {
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
            Self::ChangeView { meta, .. } => meta.clone(),
            Self::PrepareRequest { meta, .. } => meta.clone(),
            Self::PrepareResponse { meta, .. } => meta.clone(),
            Self::Commit { meta, .. } => meta.clone(),
            Self::RecoveryRequest { meta, .. } => meta.clone(),
            Self::RecoveryMessage { meta, .. } => meta.clone(),
        }
    }
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct PrepareRequest {
    pub version: u32,
    pub prev_hash: H256,

    /// i.e. timestamp
    pub unix_milli: u64,
    pub nonce: u64,
    pub tx_hashes: Vec<H256>,

    // pub state_root: H256,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct PrepareResponse {
    pub preparation: H256,
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

    /// i.e. timestamp
    pub unix_milli: u64,
    pub reason: ChangeViewReason,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct RecoveryRequest {
    /// i.e. timestamp
    pub unix_milli: u64,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct RecoveryMessage {
    pub change_views: Vec<ChangeViewCompact>,
    pub prepare_stage: PrepareStage,
    pub preparations: Vec<PreparationCompact>,
    pub commits: Vec<CommitCompact>,
}


#[derive(Debug, Clone)]
pub enum PrepareStage {
    Prepare(PrepareRequest),

    Preparation(Option<H256>),
}

impl BinEncoder for PrepareStage {
    fn encode_bin(&self, w: &mut impl BinWriter) {
        match self {
            Self::Prepare(p) => {
                0x00u8.encode_bin(w);
                p.encode_bin(w);
            }
            Self::Preparation(p) => {
                0x01u8.encode_bin(w);
                if let Some(h) = p {
                    w.write_varint_le(H256_SIZE as u64);
                    w.write(h.as_le_bytes());
                } else {
                    w.write_varint_le(0);
                }
            }
        }
    }

    fn bin_size(&self) -> usize {
        match self {
            Self::Prepare(p) => p.bin_size(),
            Self::Preparation(p) => if p.is_none() { 1 } else { 1 + H256_SIZE },
        }
    }
}

impl BinDecoder for PrepareStage {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let offset = r.consumed();
        let prepare: u8 = BinDecoder::decode_bin(r)?;
        if prepare != 0x00 && prepare != 0x01 {
            return Err(BinDecodeError::InvalidType("PrepareStage", offset, prepare as u64));
        }

        if prepare == 0x00 {
            return Ok(Self::Prepare(BinDecoder::decode_bin(r)?));
        }

        let size = r.read_varint_le()?;
        if size != 0 && size != H256_SIZE as u64 {
            return Err(BinDecodeError::InvalidValue("PrepareStage", offset + 1));
        }

        Ok(Self::Preparation(if size == 0 { None } else { Some(BinDecoder::decode_bin(r)?) }))
    }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct ChangeViewCompact {
    pub validator_index: ViewIndex,
    pub original_view_number: ViewNumber,

    /// i.e. timestamp
    pub unix_milli: u64,
    pub invocation_script: Bytes,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct CommitCompact {
    pub view_number: ViewNumber,
    pub validator_index: ViewIndex,
    pub sign: Sign,
    pub invocation_script: Bytes,
}


#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct PreparationCompact {
    pub validator_index: ViewIndex,
    pub invocation_script: Bytes,
}
