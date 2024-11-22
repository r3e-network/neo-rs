use std::convert::TryFrom;

use crate::vm::{OpCode, VMError};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Instruction {
    pub opcode:  OpCode,
    pub operand: Vec<u8>,
}

#[derive(Debug)]
enum Error {
    InvalidOpcode,
    InvalidOperandSize,
    InvalidPrefixSize(usize),
    OperandOutOfBounds {
        instruction_pointer: usize,
        operand_size:        usize,
        script_length:       usize,
    },
}
impl Instruction {
    pub const RET: Self = Self { opcode: OpCode::Ret, operand: Vec::new() };

    pub fn size(&self) -> usize {
        let prefix_size = self.opcode.operand_prefix().unwrap_or(0); //  OPERAND_SIZE_PREFIX[self.opcode as usize];
        if prefix_size > 0 {
            (1 + prefix_size + self.operand.len() as u8) as usize
        } else {
            (1 + self.opcode.operand_size().unwrap_or(0)) as usize
        }
    }

    // Token getters
    pub fn token_i8(&self) -> i8 {
        self.operand[0] as i8
    }

    pub fn token_i8_1(&self) -> i8 {
        self.operand[1] as i8
    }

    pub fn token_i32(&self) -> i32 {
        i32::from_le_bytes(self.operand[..4].try_into().unwrap())
    }

    pub fn token_i32_1(&self) -> i32 {
        i32::from_le_bytes(self.operand[4..8].try_into().unwrap())
    }

    // Other token methods
    pub fn token_u8(&self) -> u8 {
        self.operand[0]
    }

    pub fn token_u8_1(&self) -> u8 {
        self.operand[1]
    }

    pub fn token_u16(&self) -> u16 {
        u16::from_le_bytes(self.operand[..2].try_into().unwrap())
    }

    pub fn token_u32(&self) -> u32 {
        u32::from_le_bytes(self.operand[..4].try_into().unwrap())
    }

    pub fn token_string(&self) -> String {
        String::from_utf8(self.operand.clone()).unwrap()
    }
    pub fn from_script(script: &[u8], ip: usize) -> Result<Self, VMError> {
        let opcode = OpCode::try_from(script[ip]).unwrap();
        let mut ip = ip + 1;

        let mut operand_size = 0;
        let prefix_size = opcode.operand_prefix().unwrap_or(0) as usize;
        match prefix_size {
            0 => {
                operand_size = opcode.operand_size().unwrap_or(0) as usize;
            }
            1 => {
                operand_size = script[ip] as usize;
                ip += 1;
            }
            2 => {
                operand_size = u16::from_le_bytes([script[ip], script[ip + 1]]) as usize;
                ip += 2;
            }
            4 => {
                operand_size = i32::from_le_bytes([
                    script[ip],
                    script[ip + 1],
                    script[ip + 2],
                    script[ip + 3],
                ]) as usize;
                ip += 4;
            }
            _ => return Err(VMError::InvalidPrefixSize(prefix_size)),
        }

        let operand = script[ip..ip + operand_size].to_vec();
        Ok(Self { opcode, operand })
    }
}
