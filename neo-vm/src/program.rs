// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use neo_base::errors;
use neo_type::ToScriptHash;

use crate::{OpCode::*, *};

#[derive(Debug, errors::Error)]
pub enum ProgramError {
    #[error("program: {0:?}")]
    OpError(#[from] OpError),

    #[error("program: invalid jump target for {0:?} at {1} to {2}")]
    InvalidJumpTarget(OpCode, u32, u32),

    #[error("program: invalid StackItemType({2:0x}) for {0:?} at {1}")]
    InvalidStackItemType(OpCode, u32, u8),
}

#[derive(Debug, Clone)]
pub struct Op {
    pub ip:      u32,
    pub code:    OpCode,
    pub operand: Operand,
}

pub trait AsOp {
    fn as_op(&self, ip: u32) -> Op;

    fn as_operand_op(&self, ip: u32, operand: Operand) -> Op;
}

impl AsOp for OpCode {
    #[inline]
    fn as_op(&self, ip: u32) -> Op {
        Op { ip, code: *self, operand: Operand::default() }
    }

    #[inline]
    fn as_operand_op(&self, ip: u32, operand: Operand) -> Op {
        Op { ip, code: *self, operand }
    }
}

// Neo VM Program
pub struct Program {
    script_hash: ScriptHash,
    ops:         Vec<Op>,
}

impl Program {
    #[inline]
    pub fn nop() -> Self {
        Self { script_hash: [].to_script_hash(), ops: Vec::new() }
    }

    #[inline]
    pub fn script_hash(&self) -> &ScriptHash {
        &self.script_hash
    }

    #[inline]
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    pub fn build(script: &[u8]) -> Result<Program, ProgramError> {
        let mut decoder = ScriptDecoder::new(script);
        let mut ops = Vec::with_capacity(script.len() / 2);
        while let Some(op) = decoder.next() {
            ops.push(op?);
        }

        let search = |op: &Op, offset: i64| {
            let to = (op.ip as i64 + offset) as u32;
            ops.binary_search_by(|x| x.ip.cmp(&to))
                .map_err(|_| ProgramError::InvalidJumpTarget(op.code, op.ip, to))
        };

        for op in ops.iter() {
            match op.code {
                _ if op.code.as_u8() >= Jmp.as_u8() && op.code.as_u8() <= CallL.as_u8() => {
                    let _ = search(op, op.operand.first)?;
                }
                PushA | EndTry | EndTryL => {
                    let _ = search(op, op.operand.first)?;
                }
                Try | TryL => {
                    let _ = search(op, op.operand.first)?;
                    let _ = search(op, op.operand.second)?;
                }
                NewArrayT | IsType | Convert => {
                    let typ = op.operand.first as u8;
                    let _ = StackItemType::try_from(typ)
                        .map_err(|_| ProgramError::InvalidStackItemType(op.code, op.ip, typ))?;
                }
                _ => { /* Syscall => {} */ }
            }
        }

        // TODO: optimize script hash
        Ok(Program { script_hash: script.to_script_hash(), ops })
    }
}

#[cfg(test)]
mod test {
    use neo_base::encoding::hex::DecodeHex;

    use crate::{decode::test::TEST_CODES_1, *};

    #[test]
    fn test_program_build() {
        let script = TEST_CODES_1.decode_hex().expect("`decode_hex` should be ok");

        let program = Program::build(&script).expect("`Program::build` should be ok");

        assert_eq!(program.ops().is_empty(), false);
        // for op in program.ops.iter() {
        //     std::println!("{:04}: {:?}, {:?}", op.ip, op.code, op.operand);
        // }
    }
}
