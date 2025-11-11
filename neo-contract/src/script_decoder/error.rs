use neo_core::script::OpCode;

#[derive(Debug)]
pub enum ScriptDecodeError {
    InvalidOpcode { byte: u8, offset: usize },
    UnexpectedEof { opcode: OpCode, offset: usize },
    UnsupportedOpcode(OpCode),
    UnknownSyscall { hash: u32 },
}

impl core::fmt::Display for ScriptDecodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ScriptDecodeError::InvalidOpcode { byte, offset } => {
                write!(f, "invalid opcode 0x{byte:02X} at offset {offset}")
            }
            ScriptDecodeError::UnexpectedEof { opcode, offset } => write!(
                f,
                "unexpected EOF while decoding {:?} operand at offset {}",
                opcode, offset
            ),
            ScriptDecodeError::UnsupportedOpcode(op) => {
                write!(f, "unsupported opcode {:?}", op)
            }
            ScriptDecodeError::UnknownSyscall { hash } => {
                write!(f, "unsupported syscall hash 0x{hash:08X}")
            }
        }
    }
}

impl core::error::Error for ScriptDecodeError {}
