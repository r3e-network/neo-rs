// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::bytes::{PickU16, PickU32, PickU64};
use tinyvec::TinyVec;


#[derive(Debug, Copy, Clone)]
pub(crate) struct CodeAttr {
    pub price: u64,

    // data/address size after this OpCode
    pub trailing: u32,

    pub unsigned: bool,
    // pub may_jump: bool,
}

#[derive(Debug, Default, Clone)]
pub struct Operand {
    // The first operand, may convert from i8, i16, i32, i64, u8, u32
    pub first: i64,

    // The second operand, may convert from i8, i32, u8
    pub second: i64,

    // Data for PushData1, PushData2, PushData4,
    pub data: TinyVec<[u8; 32]>,
}

impl Operand {
    fn with_unary(first: i64) -> Self {
        Self { first, second: 0, data: Default::default() }
    }

    pub fn with_dual(first: i64, second: i64) -> Self {
        Self { first, second, data: Default::default() }
    }

    pub fn with_data(data: &[u8]) -> Self {
        Self { first: 0, second: 0, data: data.into() }
    }

    pub(crate) fn with_signed(operand: &[u8]) -> Self {
        let first = match operand.len() {
            1 => operand[0] as i8 as i64,
            2 => operand.pick_le_u16() as i16 as i64,
            4 => operand.pick_le_u32() as i32 as i64,
            8 => operand.pick_le_u64() as i64,
            _ => unreachable!("unexpected signed operand"),
        };
        Self::with_unary(first)
    }

    pub(crate) fn with_unsigned(operand: &[u8]) -> Self {
        let first = match operand.len() {
            1 => operand[0] as i64,
            2 => operand.pick_le_u16() as i64,
            4 => operand.pick_le_u32() as i64,
            _ => unreachable!("unexpected unsigned operand"),
        };
        Self::with_unary(first)
    }
}

#[cfg(test)]
mod test {
    use neo_core::types::{OpCode, OP_CODES};
    use strum::IntoEnumIterator;

    #[test]
    fn test_opcode_valid() {
        let mut codes = [false; 256];
        for code in OpCode::iter() {
            assert_eq!(OpCode::is_valid(code.as_u8()), true);
            codes[code.as_u8() as usize] = true;
        }

        for i in 0..codes.len() {
            assert_eq!(OP_CODES[i].is_some(), codes[i]);
        }
    }
}
