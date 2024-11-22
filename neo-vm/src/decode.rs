// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::bytes::{PickAtMost, PickU32};
use neo_base::errors;

use crate::{OpCode::*, *};

#[derive(Debug, errors::Error)]
pub enum OpError {
    #[error("op: invalid opcode '0x{1:X}' at {0}")]
    InvalidOpCode(u32, u8),

    #[error("op: param of '0x{1:X}' out-of-bound at {0}")]
    OutOfBound(u32, u8),
    // #[error("op: param of '0x{1:X}' too-large at {0}")]
    // TooLargeParam(u32, u8),
}

impl OpError {
    #[inline]
    pub fn which(&self) -> (u32, u8) {
        match self {
            Self::InvalidOpCode(ip, op) => (*ip, *op),
            Self::OutOfBound(ip, op) => (*ip, *op),
        }
    }
}

pub struct ScriptDecoder<'a> {
    next:   usize,
    script: &'a [u8],
}

impl<'a> ScriptDecoder<'a> {
    pub fn new(script: &'a [u8]) -> Self {
        Self { script, next: 0 }
    }
}

impl Iterator for ScriptDecoder<'_> {
    type Item = Result<Op, OpError>;

    // Do not call again if `next` returned Error
    fn next(&mut self) -> Option<Self::Item> {
        let ip = self.next;
        let script = self.script;
        let max_ip = script.len();
        if ip >= max_ip {
            return None;
            // return Some(Ok(Return.as_op())); // neo-go return OpCode Return on there
        }
        self.next += 1;

        let next = script[ip];
        let Some(opcode) = OpCode::from_u8(next) else {
            return Some(Err(OpError::InvalidOpCode(ip as u32, next)));
        };

        let attr = &CODE_ATTRS[next as usize];
        let trailing = attr.trailing as usize;
        if trailing <= 0 {
            return Some(Ok(opcode.as_op(ip as u32)));
        }

        self.next += trailing;
        if self.next >= max_ip {
            return Some(Err(OpError::OutOfBound(ip as u32, next)));
        }

        let op = &script[ip + 1..self.next];
        let operand = match opcode {
            PushData1 | PushData2 | PushData4 => {
                let data_len = u32::from_le_bytes(op.pick_at_most()) as usize;
                let data_ip = self.next;
                self.next += data_len; // may exceed `self.script.len()`
                if data_ip + data_len >= max_ip || data_len > MAX_STACK_ITEM_SIZE {
                    return Some(Err(OpError::OutOfBound(ip as u32, next)));
                }
                Operand::with_data(&script[data_ip..self.next])
            }
            PushInt128 | PushInt256 => Operand::with_data(op),
            Try | InitSlot => Operand::with_dual(op[0] as i64, op[1] as i64),
            TryL => Operand::with_dual(op.pick_le_u32() as i64, op[4..].pick_le_u32() as i64),
            _ => {
                if attr.unsigned {
                    Operand::with_unsigned(op)
                } else {
                    Operand::with_signed(op)
                }
            }
        };

        Some(Ok(opcode.as_operand_op(ip as u32, operand)))
    }
}

#[cfg(test)]
pub(crate) mod test {
    use neo_base::encoding::hex::DecodeHex;

    use crate::{OpCode::*, ScriptDecoder};

    pub(crate) const TEST_CODES_1: &str = "57020004ffffffffffffffff0000000000000000701071223d6801ff00a34a7045694a9c4a020000\
        00802e04220a4a02ffffff7f321e03ffffffff00000000914a02ffffff7f320c03000000000100000\
        09f714569010004b524c068220240";

    #[test]
    fn test_op_iter() {
        let script = TEST_CODES_1.decode_hex().expect("`decode_hex` should be ok");

        let mut decoder = ScriptDecoder::new(&script);
        while let Some(op) = decoder.next() {
            let op = op.expect("`op` should be ok");
            match op.code {
                PushInt128 | PushData1 | PushData2 => assert_eq!(op.operand.first, 0),
                _ => assert!(op.operand.data.is_empty()),
            }
        }
    }
}
