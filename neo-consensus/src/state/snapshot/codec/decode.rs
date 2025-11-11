use alloc::{collections::BTreeMap, vec::Vec};
use core::convert::TryFrom;

use neo_base::{encoding::DecodeError, hash::Hash256, read_varint, NeoDecode, NeoRead};

use crate::{
    message::{ChangeViewReason, MessageKind, SignedMessage, ViewNumber},
    validator::ValidatorId,
};

use super::super::model::SnapshotState;

impl NeoDecode for SnapshotState {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let height = u64::neo_decode(reader)?;
        let view = ViewNumber::neo_decode(reader)?;
        let proposal = match reader.read_u8()? {
            0 => None,
            _ => Some(Hash256::neo_decode(reader)?),
        };
        let entries = read_varint(reader)? as usize;
        let mut participation = BTreeMap::new();
        for _ in 0..entries {
            let kind = MessageKind::try_from(reader.read_u8()?)?;
            let count = read_varint(reader)? as usize;
            let mut messages = Vec::with_capacity(count);
            for _ in 0..count {
                messages.push(SignedMessage::neo_decode(reader)?);
            }
            participation.insert(kind, messages);
        }

        let mut expected = BTreeMap::new();
        if reader.remaining() > 0 {
            let count = read_varint(reader)? as usize;
            for _ in 0..count {
                let kind = MessageKind::try_from(reader.read_u8()?)?;
                let validators_len = read_varint(reader)? as usize;
                let mut validators = Vec::with_capacity(validators_len);
                for _ in 0..validators_len {
                    validators.push(ValidatorId::neo_decode(reader)?);
                }
                expected.insert(kind, validators);
            }
        }

        let mut change_view_reasons = BTreeMap::new();
        if reader.remaining() > 0 {
            let count = read_varint(reader)? as usize;
            for _ in 0..count {
                let validator = ValidatorId::neo_decode(reader)?;
                let reason = ChangeViewReason::from_u8(reader.read_u8()?)?;
                change_view_reasons.insert(validator, reason);
            }
        }

        let mut change_view_reason_counts = BTreeMap::new();
        if reader.remaining() > 0 {
            let count = read_varint(reader)? as usize;
            for _ in 0..count {
                let reason = ChangeViewReason::from_u8(reader.read_u8()?)?;
                let value = read_varint(reader)? as usize;
                change_view_reason_counts.insert(reason, value);
            }
        }
        let change_view_total = if reader.remaining() > 0 {
            reader.read_u64()?
        } else {
            0
        };

        Ok(Self {
            height,
            view,
            proposal,
            participation,
            expected,
            change_view_reasons,
            change_view_reason_counts,
            change_view_total,
        })
    }
}
