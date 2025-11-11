use alloc::{string::String, vec::Vec};

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, SliceReader},
    hash::double_sha256,
};

use crate::nef::{
    token::MethodToken,
    util::{
        validate_compiler, validate_method_name, validate_script, validate_source,
        validate_tokens_len, write_array, write_fixed_string,
    },
    COMPILER_FIELD_SIZE, NEF_MAGIC,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NefFile {
    pub compiler: String,
    pub source: String,
    pub tokens: Vec<MethodToken>,
    pub script: Vec<u8>,
    pub checksum: u32,
}

impl NefFile {
    pub fn new(
        compiler: impl Into<String>,
        source: impl Into<String>,
        tokens: Vec<MethodToken>,
        script: Vec<u8>,
    ) -> Result<Self, DecodeError> {
        let compiler = compiler.into();
        let source = source.into();
        validate_compiler(&compiler)?;
        validate_source(&source)?;
        validate_tokens_len(tokens.len())?;
        validate_script(&script)?;

        let mut nef = Self {
            compiler,
            source,
            tokens,
            script,
            checksum: 0,
        };
        nef.checksum = nef.compute_checksum();
        Ok(nef)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut reader = SliceReader::new(bytes);
        let nef = Self::neo_decode(&mut reader)?;
        if reader.remaining() != 0 {
            return Err(DecodeError::InvalidValue("NefTrailingBytes"));
        }
        Ok(nef)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_vec()
    }

    pub fn verify_checksum(&self) -> bool {
        self.compute_checksum() == self.checksum
    }

    pub(crate) fn validate(&self) -> Result<(), DecodeError> {
        validate_compiler(&self.compiler)?;
        validate_source(&self.source)?;
        validate_tokens_len(self.tokens.len())?;
        validate_script(&self.script)?;
        for token in &self.tokens {
            validate_method_name(&token.method)?;
        }
        Ok(())
    }

    fn compute_checksum(&self) -> u32 {
        let mut buf = Vec::new();
        self.encode_without_checksum(&mut buf);
        let hash = double_sha256(buf);
        u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]])
    }

    pub(crate) fn encode_without_checksum<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u32(NEF_MAGIC);
        write_fixed_string(writer, &self.compiler, COMPILER_FIELD_SIZE);
        writer.write_var_bytes(self.source.as_bytes());
        writer.write_u8(0);
        write_array(writer, &self.tokens);
        writer.write_u16(0);
        writer.write_var_bytes(&self.script);
    }
}
