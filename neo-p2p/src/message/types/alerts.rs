use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite},
    Bytes,
};

use super::super::command::MessageCommand;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectPayload {
    pub command: MessageCommand,
    pub code: u8,
    pub reason: String,
    pub data: Bytes,
}

impl NeoEncode for RejectPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(self.command as u8);
        writer.write_u8(self.code);
        self.reason.neo_encode(writer);
        self.data.neo_encode(writer);
    }
}

impl NeoDecode for RejectPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let command = MessageCommand::try_from(reader.read_u8()?)?;
        let code = reader.read_u8()?;
        let reason = String::neo_decode(reader)?;
        let data = Bytes::neo_decode(reader)?;
        Ok(Self {
            command,
            code,
            reason,
            data,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlertPayload {
    pub data: Bytes,
}

impl NeoEncode for AlertPayload {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.data.neo_encode(writer);
    }
}

impl NeoDecode for AlertPayload {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Self {
            data: Bytes::neo_decode(reader)?,
        })
    }
}
