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
#[path = "../../tests/protocol/oracle/oracle_response_code.rs"]
mod tests;
