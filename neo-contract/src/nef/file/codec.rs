use crate::nef::{
    util::{read_array, read_limited_string},
    COMPILER_FIELD_SIZE, MAX_SCRIPT_SIZE, NEF_MAGIC, SOURCE_URL_MAX,
};
use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use super::model::NefFile;

impl NeoEncode for NefFile {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.validate()
            .expect("valid NEF file before encoding (internal invariant)");
        self.encode_without_checksum(writer);
        writer.write_u32(self.checksum);
    }
}

impl NeoDecode for NefFile {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let initial_remaining = reader.remaining();
        let magic = reader.read_u32()?;
        if magic != NEF_MAGIC {
            return Err(DecodeError::InvalidValue("NefMagic"));
        }
        let compiler = read_limited_string(reader, COMPILER_FIELD_SIZE, "Nef.compiler")?;
        let source = read_limited_string(reader, SOURCE_URL_MAX, "Nef.source")?;
        reader.read_u8()?; // reserved
        let tokens = read_array(reader)?;
        let reserved = reader.read_u16()?;
        if reserved != 0 {
            return Err(DecodeError::InvalidValue("Nef.reserved"));
        }
        let script = reader.read_var_bytes(MAX_SCRIPT_SIZE)?;
        let checksum = reader.read_u32()?;
        let consumed = initial_remaining - reader.remaining();
        if consumed as u64 > MAX_SCRIPT_SIZE + 128 * 64 {
            return Err(DecodeError::LengthOutOfRange {
                len: consumed as u64,
                max: MAX_SCRIPT_SIZE + 128 * 64,
            });
        }
        let nef = Self {
            compiler,
            source,
            tokens,
            script,
            checksum,
        };
        if !nef.verify_checksum() {
            return Err(DecodeError::InvalidValue("NefChecksum"));
        }
        Ok(nef)
    }
}
