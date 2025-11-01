// Copyright (C) 2015-2025 The Neo Project.
//
// consensus_message_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Consensus message type enum matching C# ConsensusMessageType exactly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsensusMessageType {
    /// Change view message
    ChangeView = 0x00,
    /// Prepare request message
    PrepareRequest = 0x20,
    /// Prepare response message
    PrepareResponse = 0x21,
    /// Commit message
    Commit = 0x30,
    /// Recovery request message
    RecoveryRequest = 0x40,
    /// Recovery message
    RecoveryMessage = 0x41,
}

impl ConsensusMessageType {
    /// Converts from byte value
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::ChangeView),
            0x20 => Some(Self::PrepareRequest),
            0x21 => Some(Self::PrepareResponse),
            0x30 => Some(Self::Commit),
            0x40 => Some(Self::RecoveryRequest),
            0x41 => Some(Self::RecoveryMessage),
            _ => None,
        }
    }

    /// Converts to byte value
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}
