use neo_base::encoding::StartsWith0x;

use super::{H160, H160_SIZE};

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum ToH160Error {
    #[error("to-h160: hex-encode H160's length must be 40(without '0x')")]
    InvalidLength,

    #[error("to-h160: invalid character '{0}'")]
    InvalidChar(char),
}

impl TryFrom<&str> for H160 {
    type Error = ToH160Error;

    /// Value must be big-endian (Neo address-style string).
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() {
            &value[2..]
        } else {
            value
        };

        if value.len() != H160_SIZE * 2 {
            return Err(ToH160Error::InvalidLength);
        }

        let mut buf = [0u8; H160_SIZE];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|err| match err {
            hex::FromHexError::InvalidHexCharacter { c, .. } => ToH160Error::InvalidChar(c),
            _ => ToH160Error::InvalidLength,
        })?;

        buf.reverse();
        Ok(H160::from_le_bytes(buf))
    }
}
