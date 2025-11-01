// Copyright (C) 2015-2025 The Neo Project.
//
// oracle_response_code.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Represents the response code for the oracle request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// Convert from byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
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
}
