// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::bytes::{PickAtMost, PickU32};
use neo_base::errors;

use crate::{*, OpCode::*};


#[derive(Debug, errors::Error)]
pub enum OpError {
    #[error("op: invalid opcode '0x{1:X}' at {0}")]
    InvalidOpCode(u32, u8),

    #[error("op: param of '0x{1:X}' out-of-bound at {0}")]
    OutOfBound(u32, u8),

    // #[error("op: param of '0x{1:X}' too-large at {0}")]
    // TooLargeParam(u32, u8),
}

pub struct ScriptDecoder<'a> {
    next: usize,
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

        let attr = opcode.attr();
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
                self.next += data_len;
                if data_ip + data_len >= max_ip || data_len > MAX_STACK_ITEM_SIZE {
                    return Some(Err(OpError::OutOfBound(ip as u32, next)));
                }
                Operand::with_data(&script[data_ip..self.next])
            }
            PushInt128 | PushInt256 => { Operand::with_data(op) }
            Try | InitSlot => { Operand::with_dual(op[0] as i64, op[1] as i64) }
            TryL => { Operand::with_dual(op.pick_le_u32() as i64, op[4..].pick_le_u32() as i64) }
            _ => { if attr.unsigned { Operand::with_unsigned(op) } else { Operand::with_signed(op) } }
        };

        Some(Ok(opcode.as_operand_op(ip as u32, operand)))
    }
}


#[cfg(test)]
pub(crate) mod test {
    use neo_base::encoding::hex::DecodeHex;
    use crate::{ScriptDecoder, OpCode::*};

    pub(crate) const TEST_CODES_1: &str =
        "56020b600c0061400b4057090010c0700c1461616161616161616161616161616161616161610c076d6574686f\
        64311f685441627d5b524510c0710c1461616161616161616161616161616161616161610c076d6574686f64321f\
        695441627d5b524510c0720c1461616161616161616161616161616161616161610c076d6574686f64321f6a5441\
        627d5b524510c0730c1461616161616161616161616161616161616161610c076d6574686f6433116b5441627d5b\
        524510c0740c146161616161616161616161616161616161616161591f6c5441627d5b524510c075580c0a736f6d\
        654d6574686f641f6d5441627d5b524510c076584ad9282404db280c0a736f6d654d6574686f641f6e5441627d5b\
        524535f3feffff770710c077086f070c076d6574686f64341f6f085441627d5b524540";

    #[test]
    fn test_op_iter() {
        let script = TEST_CODES_1.decode_hex()
            .expect("`decode_hex` should be ok");

        let mut decoder = ScriptDecoder::new(&script);
        while let Some(op) = decoder.next() {
            let op = op.expect("`op` should be ok");
            match op.code {
                PushData1 | PushData2 | PushData4 => { assert_eq!(op.operand.first, 0); }
                _ => { assert!(op.operand.data.is_empty()); }
            }
            // std::println!("{:04}: {:?}, {:?}", op.ip, op.code, op.operand);
        }
    }
}