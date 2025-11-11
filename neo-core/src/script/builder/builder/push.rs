use crate::script::{opcode::OpCode, MAX_SCRIPT_LENGTH};

use super::super::utils::{minimal_signed_bytes, syscall_hash};
use super::ScriptBuilder;

impl ScriptBuilder {
    pub fn push_data(&mut self, data: &[u8]) -> &mut Self {
        let len = data.len();
        assert!(
            len <= MAX_SCRIPT_LENGTH,
            "script literal exceeds maximum size"
        );
        if len <= u8::MAX as usize {
            self.push_opcode(OpCode::PushData1);
            self.script.push(len as u8);
        } else if len <= u16::MAX as usize {
            self.push_opcode(OpCode::PushData2);
            self.script.extend_from_slice(&(len as u16).to_le_bytes());
        } else {
            self.push_opcode(OpCode::PushData4);
            self.script.extend_from_slice(&(len as u32).to_le_bytes());
        }
        self.script.extend_from_slice(data);
        self
    }

    pub fn push_string(&mut self, value: &str) -> &mut Self {
        self.push_data(value.as_bytes())
    }

    pub fn push_bool(&mut self, value: bool) -> &mut Self {
        if value {
            self.push_opcode(OpCode::PushTrue)
        } else {
            self.push_opcode(OpCode::PushFalse)
        }
    }

    pub fn push_int(&mut self, value: i64) -> &mut Self {
        match value {
            -1 => return self.push_opcode(OpCode::PushM1),
            0 => return self.push_opcode(OpCode::Push0),
            1..=16 => {
                let opcode = OpCode::Push1 as u8 + (value as u8 - 1);
                self.script.push(opcode);
                return self;
            }
            _ => {}
        }

        let mut data = minimal_signed_bytes(value);
        if data.is_empty() {
            data.push(0);
        }
        self.push_data(&data)
    }

    pub fn push_syscall(&mut self, method: &str) -> &mut Self {
        let hash = syscall_hash(method);
        self.push_opcode(OpCode::Syscall);
        self.script.extend_from_slice(&hash.to_le_bytes());
        self
    }
}
