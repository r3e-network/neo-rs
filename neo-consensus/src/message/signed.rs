use alloc::vec::Vec;

use neo_base::{double_sha256, hash::Hash256, NeoDecode, NeoEncode, NeoRead, NeoWrite};
use neo_crypto::SignatureBytes;

use crate::validator::ValidatorId;

use super::{
    payload::ConsensusMessage,
    types::{MessageKind, ViewNumber},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedMessage {
    pub height: u64,
    pub view: ViewNumber,
    pub validator: ValidatorId,
    pub message: ConsensusMessage,
    pub signature: SignatureBytes,
}

impl SignedMessage {
    pub fn new(
        height: u64,
        view: ViewNumber,
        validator: ValidatorId,
        message: ConsensusMessage,
        signature: SignatureBytes,
    ) -> Self {
        Self {
            height,
            view,
            validator,
            message,
            signature,
        }
    }

    pub fn kind(&self) -> MessageKind {
        self.message.kind()
    }

    pub fn digest(&self) -> Hash256 {
        let mut buf = Vec::new();
        UnsignedEnvelope::from(self).neo_encode(&mut buf);
        Hash256::new(double_sha256(buf))
    }
}

struct UnsignedEnvelope<'a> {
    height: u64,
    view: ViewNumber,
    validator: ValidatorId,
    message: &'a ConsensusMessage,
}

impl<'a> From<&'a SignedMessage> for UnsignedEnvelope<'a> {
    fn from(value: &'a SignedMessage) -> Self {
        Self {
            height: value.height,
            view: value.view,
            validator: value.validator,
            message: &value.message,
        }
    }
}

impl<'a> NeoEncode for UnsignedEnvelope<'a> {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.height.neo_encode(writer);
        self.view.neo_encode(writer);
        self.validator.neo_encode(writer);
        self.message.neo_encode(writer);
    }
}

impl NeoEncode for SignedMessage {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        UnsignedEnvelope::from(self).neo_encode(writer);
        self.signature.neo_encode(writer);
    }
}

impl NeoDecode for SignedMessage {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, neo_base::encoding::DecodeError> {
        let height = u64::neo_decode(reader)?;
        let view = ViewNumber::neo_decode(reader)?;
        let validator = ValidatorId::neo_decode(reader)?;
        let message = ConsensusMessage::neo_decode(reader)?;
        let signature = SignatureBytes::neo_decode(reader)?;
        Ok(Self {
            height,
            view,
            validator,
            message,
            signature,
        })
    }
}
