//! `OracleResponseCode` - matches C# Neo.Network.P2P.Payloads.OracleResponseCode exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Represents the response code for the oracle request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OracleResponseCode {
    /// Indicates that the request has been successfully completed.
    Success = 0x00,
    /// Indicates that the protocol of the request is not supported.
    ProtocolNotSupported = 0x10,
    /// Indicates that the oracle nodes cannot reach a consensus on the result of the request.
    ConsensusUnreachable = 0x12,
    /// Indicates that the requested Uri does not exist.
    NotFound = 0x14,
    /// Indicates that the request was not completed within the specified time.
    Timeout = 0x16,
    /// Indicates that there is no permission to request the resource.
    Forbidden = 0x18,
    /// Indicates that the data for the response is too large.
    ResponseTooLarge = 0x1a,
    /// Indicates that the request failed due to insufficient balance.
    InsufficientFunds = 0x1c,
    /// Indicates that the content-type of the request is not supported.
    ContentTypeNotSupported = 0x1f,
    /// Indicates that the request failed due to other errors.
    Error = 0xff,
}

impl OracleResponseCode {
    /// Converts to byte representation.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    #[must_use]
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Success),
            0x10 => Some(Self::ProtocolNotSupported),
            0x12 => Some(Self::ConsensusUnreachable),
            0x14 => Some(Self::NotFound),
            0x16 => Some(Self::Timeout),
            0x18 => Some(Self::Forbidden),
            0x1a => Some(Self::ResponseTooLarge),
            0x1c => Some(Self::InsufficientFunds),
            0x1f => Some(Self::ContentTypeNotSupported),
            0xff => Some(Self::Error),
            _ => None,
        }
    }

    /// Returns the string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::ProtocolNotSupported => "ProtocolNotSupported",
            Self::ConsensusUnreachable => "ConsensusUnreachable",
            Self::NotFound => "NotFound",
            Self::Timeout => "Timeout",
            Self::Forbidden => "Forbidden",
            Self::ResponseTooLarge => "ResponseTooLarge",
            Self::InsufficientFunds => "InsufficientFunds",
            Self::ContentTypeNotSupported => "ContentTypeNotSupported",
            Self::Error => "Error",
        }
    }

    /// Returns true if this response code indicates success.
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Success)
    }

    /// Returns true if this response code indicates an error.
    #[must_use]
    pub fn is_error(self) -> bool {
        !self.is_success()
    }
}

impl fmt::Display for OracleResponseCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for OracleResponseCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for OracleResponseCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        Self::from_byte(byte).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid oracle response code byte: {byte}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_response_code_values() {
        assert_eq!(OracleResponseCode::Success.to_byte(), 0x00);
        assert_eq!(OracleResponseCode::ProtocolNotSupported.to_byte(), 0x10);
        assert_eq!(OracleResponseCode::ConsensusUnreachable.to_byte(), 0x12);
        assert_eq!(OracleResponseCode::NotFound.to_byte(), 0x14);
        assert_eq!(OracleResponseCode::Timeout.to_byte(), 0x16);
        assert_eq!(OracleResponseCode::Forbidden.to_byte(), 0x18);
        assert_eq!(OracleResponseCode::ResponseTooLarge.to_byte(), 0x1a);
        assert_eq!(OracleResponseCode::InsufficientFunds.to_byte(), 0x1c);
        assert_eq!(OracleResponseCode::ContentTypeNotSupported.to_byte(), 0x1f);
        assert_eq!(OracleResponseCode::Error.to_byte(), 0xff);
    }

    #[test]
    fn test_oracle_response_code_from_byte() {
        assert_eq!(
            OracleResponseCode::from_byte(0x00),
            Some(OracleResponseCode::Success)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x10),
            Some(OracleResponseCode::ProtocolNotSupported)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x12),
            Some(OracleResponseCode::ConsensusUnreachable)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x14),
            Some(OracleResponseCode::NotFound)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x16),
            Some(OracleResponseCode::Timeout)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x18),
            Some(OracleResponseCode::Forbidden)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x1a),
            Some(OracleResponseCode::ResponseTooLarge)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x1c),
            Some(OracleResponseCode::InsufficientFunds)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0x1f),
            Some(OracleResponseCode::ContentTypeNotSupported)
        );
        assert_eq!(
            OracleResponseCode::from_byte(0xff),
            Some(OracleResponseCode::Error)
        );
        assert_eq!(OracleResponseCode::from_byte(0x99), None);
    }

    #[test]
    fn test_oracle_response_code_roundtrip() {
        for code in [
            OracleResponseCode::Success,
            OracleResponseCode::ProtocolNotSupported,
            OracleResponseCode::ConsensusUnreachable,
            OracleResponseCode::NotFound,
            OracleResponseCode::Timeout,
            OracleResponseCode::Forbidden,
            OracleResponseCode::ResponseTooLarge,
            OracleResponseCode::InsufficientFunds,
            OracleResponseCode::ContentTypeNotSupported,
            OracleResponseCode::Error,
        ] {
            let byte = code.to_byte();
            let recovered = OracleResponseCode::from_byte(byte);
            assert_eq!(recovered, Some(code));
        }
    }

    #[test]
    fn test_oracle_response_code_display() {
        assert_eq!(OracleResponseCode::Success.to_string(), "Success");
        assert_eq!(OracleResponseCode::NotFound.to_string(), "NotFound");
        assert_eq!(OracleResponseCode::Error.to_string(), "Error");
    }

    #[test]
    fn test_oracle_response_code_is_success() {
        assert!(OracleResponseCode::Success.is_success());
        assert!(!OracleResponseCode::NotFound.is_success());
        assert!(!OracleResponseCode::Error.is_success());
    }

    #[test]
    fn test_oracle_response_code_is_error() {
        assert!(!OracleResponseCode::Success.is_error());
        assert!(OracleResponseCode::NotFound.is_error());
        assert!(OracleResponseCode::Timeout.is_error());
        assert!(OracleResponseCode::Error.is_error());
    }

    #[test]
    fn test_oracle_response_code_serde() {
        let code = OracleResponseCode::NotFound;
        let serialized = serde_json::to_string(&code).unwrap();
        assert_eq!(serialized, "20"); // 0x14 = 20

        let deserialized: OracleResponseCode = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, code);
    }
}
