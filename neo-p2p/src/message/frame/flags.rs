use neo_base::encoding::DecodeError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageFlags(u8);

impl MessageFlags {
    pub const NONE: Self = Self(0);
    pub const COMPRESSED: Self = Self(1);

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn insert(&mut self, other: Self) {
        self.0 |= other.0;
    }

    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    pub fn from_bits(bits: u8) -> Result<Self, DecodeError> {
        if bits & !Self::COMPRESSED.0 != 0 {
            return Err(DecodeError::InvalidValue("message flags"));
        }
        Ok(Self(bits))
    }
}
