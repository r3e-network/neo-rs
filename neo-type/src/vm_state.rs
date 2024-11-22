use neo_base::encoding::bin::{BinDecode, BinEncode};

#[derive(Debug, Copy, Clone, Eq, PartialEq, BinEncode, BinDecode)]
#[bin(repr = u8)]
pub enum VMState {
    None  = 0,
    Halt  = 1,
    Fault = 2,
    Break = 4,
}
