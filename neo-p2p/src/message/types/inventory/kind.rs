use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::message::command::MessageCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryKind {
    Transaction,
    Block,
    Extensible,
}

impl InventoryKind {
    fn from_byte(byte: u8) -> Result<Self, DecodeError> {
        match byte {
            x if x == MessageCommand::Transaction as u8 => Ok(Self::Transaction),
            x if x == MessageCommand::Block as u8 => Ok(Self::Block),
            x if x == MessageCommand::Extensible as u8 => Ok(Self::Extensible),
            _ => Err(DecodeError::InvalidValue("inventory kind")),
        }
    }

    fn as_byte(self) -> u8 {
        match self {
            Self::Transaction => MessageCommand::Transaction as u8,
            Self::Block => MessageCommand::Block as u8,
            Self::Extensible => MessageCommand::Extensible as u8,
        }
    }
}

impl NeoEncode for InventoryKind {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.as_byte());
    }
}

impl NeoDecode for InventoryKind {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let byte = reader.read_u8()?;
        Self::from_byte(byte)
    }
}
