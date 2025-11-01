// Copyright (C) 2015-2025 The Neo Project.
//
// error.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::fmt;
use std::net::SocketAddr;

/// Network-related errors.
#[derive(Debug, Clone)]
pub enum NetworkError {
    /// Protocol violation by a peer.
    ProtocolViolation {
        /// The peer that violated the protocol.
        peer: SocketAddr,
        /// Description of the violation.
        violation: String,
    },

    /// Invalid message format.
    InvalidMessage(String),

    /// Connection error.
    ConnectionError(String),

    /// Timeout error.
    Timeout,

    /// Other network error.
    Other(String),
}

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProtocolViolation { peer, violation } => {
                write!(f, "Protocol violation from {}: {}", peer, violation)
            }
            Self::InvalidMessage(msg) => write!(f, "Invalid message: {}", msg),
            Self::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            Self::Timeout => write!(f, "Network timeout"),
            Self::Other(msg) => write!(f, "Network error: {}", msg),
        }
    }
}

impl std::error::Error for NetworkError {}

/// Result type for network operations.
pub type NetworkResult<T> = Result<T, NetworkError>;
