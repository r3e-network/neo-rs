use num_traits::FromPrimitive;

/// Represents the different hardforks of the Neo blockchain.
#[derive( Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Hardfork {
    HF_Aspidochelone = 0,
    HF_Basilisk = 1,
    HF_Cockatrice = 2,
    HF_Domovoi = 3,
}

impl Hardfork {
    /// Converts a u8 to a Hardfork enum value.
    pub fn from_u8(value: u8) -> Option<Hardfork> {
        FromPrimitive::from_u8(value)
    }
}
