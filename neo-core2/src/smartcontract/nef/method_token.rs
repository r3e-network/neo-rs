use std::error::Error;
use std::fmt;
use crate::util::Uint160;
use crate::smartcontract::callflag::CallFlag;
use crate::io::{BinReader, BinWriter};

// Maximum length of method name
const MAX_METHOD_LENGTH: usize = 32;

#[derive(Debug)]
struct InvalidMethodNameError;

impl fmt::Display for InvalidMethodNameError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "method name shouldn't start with '_'")
    }
}

impl Error for InvalidMethodNameError {}

#[derive(Debug)]
struct InvalidCallFlagError;

impl fmt::Display for InvalidCallFlagError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "invalid call flag")
    }
}

impl Error for InvalidCallFlagError {}

/// MethodToken is contract method description.
#[derive(Debug, Clone)]
pub struct MethodToken {
    /// Hash is contract hash.
    pub hash: Uint160,
    /// Method is method name.
    pub method: String,
    /// ParamCount is method parameter count.
    pub param_count: u16,
    /// HasReturn is true if method returns value.
    pub has_return: bool,
    /// CallFlag is a set of call flags the method will be called with.
    pub call_flag: CallFlag,
}

impl MethodToken {
    pub fn encode_binary(&self, writer: &mut BinWriter) {
        writer.write_bytes(&self.hash);
        writer.write_string(&self.method);
        writer.write_u16_le(self.param_count);
        writer.write_bool(self.has_return);
        writer.write_u8(self.call_flag as u8);
    }

    pub fn decode_binary(reader: &mut BinReader) -> Result<Self, Box<dyn Error>> {
        let mut hash = Uint160::default();
        reader.read_bytes(&mut hash)?;
        let method = reader.read_string(MAX_METHOD_LENGTH)?;
        if method.starts_with('_') {
            return Err(Box::new(InvalidMethodNameError));
        }
        let param_count = reader.read_u16_le()?;
        let has_return = reader.read_bool()?;
        let call_flag = CallFlag::from(reader.read_u8()?);
        if call_flag & !CallFlag::All != CallFlag::None {
            return Err(Box::new(InvalidCallFlagError));
        }
        Ok(MethodToken {
            hash,
            method,
            param_count,
            has_return,
            call_flag,
        })
    }
}
