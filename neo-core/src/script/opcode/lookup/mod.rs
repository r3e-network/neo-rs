use core::convert::TryFrom;

use super::OpCode;

mod generated;

impl OpCode {
    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    #[inline]
    pub const fn from_u8(code: u8) -> Option<Self> {
        generated::lookup(code)
    }

    #[inline]
    pub const fn is_valid(code: u8) -> bool {
        Self::from_u8(code).is_some()
    }
}

impl TryFrom<u8> for OpCode {
    type Error = ();

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        OpCode::from_u8(value).ok_or(())
    }
}
