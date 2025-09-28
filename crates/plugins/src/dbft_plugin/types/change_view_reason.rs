// Copyright (C) 2015-2025 The Neo Project.
//
// change_view_reason.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Change view reason enum matching C# ChangeViewReason exactly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeViewReason {
    /// Timeout occurred
    Timeout = 0x0,
    /// Change agreement
    ChangeAgreement = 0x1,
    /// Transaction not found
    TxNotFound = 0x2,
    /// Transaction rejected by policy
    TxRejectedByPolicy = 0x3,
    /// Transaction invalid
    TxInvalid = 0x4,
    /// Block rejected by policy
    BlockRejectedByPolicy = 0x5,
}

impl ChangeViewReason {
    /// Converts from byte value
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x0 => Some(Self::Timeout),
            0x1 => Some(Self::ChangeAgreement),
            0x2 => Some(Self::TxNotFound),
            0x3 => Some(Self::TxRejectedByPolicy),
            0x4 => Some(Self::TxInvalid),
            0x5 => Some(Self::BlockRejectedByPolicy),
            _ => None,
        }
    }
    
    /// Converts to byte value
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}