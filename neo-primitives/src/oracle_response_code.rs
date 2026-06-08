//! `OracleResponseCode` - matches C# Neo.Network.P2P.Payloads.OracleResponseCode exactly.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// Represents the response code for the oracle request.
    pub OracleResponseCode {
        /// The oracle request completed successfully.
        Success = 0x00,
        /// The requested protocol is not supported.
        ProtocolNotSupported = 0x10,
        /// Consensus nodes could not agree on the response.
        ConsensusUnreachable = 0x12,
        /// The requested resource was not found.
        NotFound = 0x14,
        /// The oracle request timed out.
        Timeout = 0x16,
        /// Access to the requested resource was forbidden.
        Forbidden = 0x18,
        /// The response exceeded the maximum allowed size.
        ResponseTooLarge = 0x1a,
        /// The oracle request could not be paid for.
        InsufficientFunds = 0x1c,
        /// The response content type is not supported.
        ContentTypeNotSupported = 0x1f,
        /// An unspecified oracle error occurred.
        Error = 0xff,
    }
}

impl OracleResponseCode {
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

    #[test]
    fn protocol_enum_guard_rejects_unknown_oracle_response_code_bytes() {
        assert_eq!(
            OracleResponseCode::from_byte(0xff),
            Some(OracleResponseCode::Error)
        );
        assert_eq!(OracleResponseCode::from_byte(0x11), None);
        assert_eq!(OracleResponseCode::from_byte(0xfe), None);
        assert!(serde_json::from_str::<OracleResponseCode>("17").is_err());
        assert!(serde_json::from_str::<OracleResponseCode>("254").is_err());
    }
}
