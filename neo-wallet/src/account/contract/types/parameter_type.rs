use crate::error::WalletError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ContractParameterType {
    Signature = 0x00,
    Boolean = 0x01,
    Integer = 0x02,
    Hash160 = 0x03,
    Hash256 = 0x04,
    ByteArray = 0x05,
    PublicKey = 0x06,
    String = 0x07,
    Array = 0x10,
    InteropInterface = 0x11,
    Void = 0xFF,
}

impl From<ContractParameterType> for u8 {
    fn from(value: ContractParameterType) -> Self {
        value as u8
    }
}

impl TryFrom<u8> for ContractParameterType {
    type Error = WalletError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use ContractParameterType::*;
        let ty = match value {
            0x00 => Signature,
            0x01 => Boolean,
            0x02 => Integer,
            0x03 => Hash160,
            0x04 => Hash256,
            0x05 => ByteArray,
            0x06 => PublicKey,
            0x07 => String,
            0x10 => Array,
            0x11 => InteropInterface,
            0xFF => Void,
            _ => return Err(WalletError::InvalidNep6("unknown parameter type")),
        };
        Ok(ty)
    }
}
