use crate::crypto::keys;
use crate::io::{self, Serializable};

/// Vote represents a vote message.
#[derive(Debug, Clone)]
pub struct Vote {
    pub validator_index: i32,
    pub height: u32,
    pub signature: Vec<u8>,
}

impl Serializable for Vote {
    fn encode(&self, writer: &mut io::BinWriter) -> io::Result<()> {
        writer.write_u32_le(self.validator_index as u32)?;
        writer.write_u32_le(self.height)?;
        writer.write_var_bytes(&self.signature)?;
        Ok(())
    }

    fn decode(&mut self, reader: &mut io::BinReader) -> io::Result<()> {
        self.validator_index = reader.read_u32_le()? as i32;
        self.height = reader.read_u32_le()?;
        self.signature = reader.read_var_bytes(keys::SIGNATURE_LEN)?;
        Ok(())
    }
}
