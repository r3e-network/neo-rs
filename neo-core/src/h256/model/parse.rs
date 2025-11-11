use neo_base::encoding::StartsWith0x;

use super::definition::{H256, H256_SIZE};

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum ToH256Error {
    #[error("to-h256: hex-encode H256's length must be 64(without '0x')")]
    InvalidLength,

    #[error("to-h256: invalid character '{0}'")]
    InvalidChar(char),
}

impl TryFrom<&str> for H256 {
    type Error = ToH256Error;

    /// Value must be big-endian.
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let value = value.trim_matches('"');
        let value = if value.starts_with_0x() {
            &value[2..]
        } else {
            value
        };

        if value.len() != H256_SIZE * 2 {
            return Err(Self::Error::InvalidLength);
        }

        let mut buf = [0u8; H256_SIZE];
        let _ = hex::decode_to_slice(value, &mut buf).map_err(|err| match err {
            hex::FromHexError::InvalidHexCharacter { c, .. } => Self::Error::InvalidChar(c),
            _ => Self::Error::InvalidLength,
        })?;

        buf.reverse();
        Ok(Self::from_le_bytes(buf))
    }
}
