// Copyright (C) 2015-2025 The Neo Project.
//
// transaction_attribute_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Represents the type of a TransactionAttribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum TransactionAttributeType {
    /// Indicates that the transaction is of high priority.
    HighPriority = 0x01,

    /// Indicates that the transaction is an oracle response.
    OracleResponse = 0x11,

    /// Indicates that the transaction is not valid before the specified block height.
    NotValidBefore = 0x20,

    /// Indicates that the transaction conflicts with the specified transaction.
    Conflicts = 0x21,

    /// Indicates that the transaction is notary assisted.
    NotaryAssisted = 0x22,
}

impl TransactionAttributeType {
    /// Convert from byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::HighPriority),
            0x11 => Some(Self::OracleResponse),
            0x20 => Some(Self::NotValidBefore),
            0x21 => Some(Self::Conflicts),
            0x22 => Some(Self::NotaryAssisted),
            _ => None,
        }
    }
}

impl std::fmt::Display for TransactionAttributeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::HighPriority => "HighPriority",
            Self::OracleResponse => "OracleResponse",
            Self::NotValidBefore => "NotValidBefore",
            Self::Conflicts => "Conflicts",
            Self::NotaryAssisted => "NotaryAssisted",
        };
        write!(f, "{}", s)
    }
}
