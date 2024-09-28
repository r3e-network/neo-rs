// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;
use core::fmt::Debug;

use neo_base::encoding::bin::*;
use neo_core::types::{Bytes, H256, H256_SIZE, Sign};

use crate::dbft_v2::*;

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct RecoveryRequest {
    pub unix_milli: u64,

    /// Extensible hash that contains this PrepareRequest and zero means no such value.
    #[bin(ignore)]
    pub payload_hash: H256,
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct RecoveryMessage {
    pub change_views:  Vec<ChangeViewCompact>,
    pub prepare_stage: PrepareStage,
    pub preparations:  Vec<PreparationCompact>,
    pub commits:       Vec<CommitCompact>,
}

impl RecoveryMessage {
    pub fn prepare_request(&self, meta: MessageMeta) -> Option<Message<PrepareRequest>> {
        match &self.prepare_stage {
            PrepareStage::Prepare(req) => Some(Message { meta, message: req.clone() }),
            _ => None,
        }
    }

    pub fn prepare_responses(
        &self,
        block_index: u32,
        view_number: ViewNumber,
    ) -> Vec<Message<PrepareResponse>> {
        let PrepareStage::Preparation(Some(ref hash)) = self.prepare_stage else {
            return Vec::new();
        };

        self.preparations
            .iter()
            .map(|p| p.to_prepare_response(block_index, view_number, hash.clone()))
            .collect()
    }

    pub fn commits(&self, block_index: u32) -> Vec<Message<Commit>> {
        self.commits.iter().map(|cc| cc.to_commit(block_index)).collect()
    }

    pub fn change_views(&self, block_index: u32) -> Vec<Message<ChangeViewRequest>> {
        self.change_views.iter().map(|cv| cv.to_change_view_request(block_index)).collect()
    }
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
            Self::Preparation(p) => {
                if p.is_none() {
                    1
                } else {
                    1 + H256_SIZE
                }
            }
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

        Ok(Self::Preparation(if size == 0 {
            None
        } else {
            Some(BinDecoder::decode_bin(r)?)
        }))
    }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct ChangeViewCompact {
    pub validator_index:      ViewIndex,
    pub original_view_number: ViewNumber,
    pub unix_milli:           u64,
    pub invocation_script:    Bytes,
}

impl ChangeViewCompact {
    pub fn to_change_view_request(&self, block_index: u32) -> Message<ChangeViewRequest> {
        Message {
            meta:    MessageMeta {
                block_index,
                validator_index: self.validator_index,
                view_number: self.original_view_number,
            },
            message: ChangeViewRequest {
                new_view_number: self.original_view_number + 1,
                unix_milli:      self.unix_milli,
                reason:          ChangeViewReason::Unknown,
            },
        }
    }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct CommitCompact {
    pub view_number: ViewNumber,
    pub validator_index: ViewIndex,
    pub sign: Sign,
    pub invocation_script: Bytes,
}

impl CommitCompact {
    pub fn to_commit(&self, block_index: u32) -> Message<Commit> {
        Message {
            meta:    MessageMeta {
                block_index,
                validator_index: self.validator_index,
                view_number: self.view_number,
            },
            message: Commit { sign: self.sign.clone() },
        }
    }
}

#[derive(Debug, Clone, BinEncode, BinDecode)]
pub struct PreparationCompact {
    pub validator_index:   ViewIndex,
    pub invocation_script: Bytes,
}

impl PreparationCompact {
    pub fn to_prepare_response(
        &self,
        block_index: u32,
        view_number: ViewNumber,
        preparation: H256,
    ) -> Message<PrepareResponse> {
        let validator_index = self.validator_index;
        Message {
            meta:    MessageMeta { block_index, validator_index, view_number },
            message: PrepareResponse { preparation },
        }
    }
}
