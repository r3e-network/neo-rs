#[derive(Debug, Clone, thiserror::Error)]
pub enum AddressError {
    #[error("address: invalid length {length}, expected 21 bytes (version + script hash)")]
    InvalidLength { length: usize },

    #[error("address: invalid version byte (expected 0x{expected:02X}, found 0x{found:02X})")]
    InvalidVersion { expected: u8, found: u8 },

    #[error("address: {0}")]
    Base58(#[from] crate::encoding::FromBase58CheckError),
}

/// Wrapper around the Neo protocol address version byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AddressVersion(pub u8);

impl AddressVersion {
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Mainnet address version (0x35 in C# `ProtocolSettings`).
    pub const MAINNET: Self = Self(0x35);

    /// Testnet address version (0x23 in C# `ProtocolSettings`).
    pub const TESTNET: Self = Self(0x23);
}

impl Default for AddressVersion {
    fn default() -> Self {
        Self::MAINNET
    }
}
