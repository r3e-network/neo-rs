// Copyright (C) 2015-2024 The Neo Project.
//
// verify_result.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::*;

/// Represents a verifying result of an inventory item.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// Indicates that the verification was successful.
    Succeed = 0,

    /// Indicates that an inventory item with the same hash already exists.
    AlreadyExists = 1,

    /// Indicates that an inventory item with the same hash already exists in the memory pool.
    AlreadyInPool = 2,

    /// Indicates that the memory pool is full and the transaction cannot be verified.
    OutOfMemory = 3,

    /// Indicates that the previous block of the current block has not been received, so the block cannot be verified.
    UnableToVerify = 4,

    /// Indicates that the inventory item is invalid.
    Invalid = 5,

    /// Indicates that the transaction has an invalid script.
    InvalidScript = 6,

    /// Indicates that the transaction has an invalid attribute.
    InvalidAttribute = 7,

    /// Indicates that the inventory item has an invalid signature.
    InvalidSignature = 8,

    /// Indicates that the size of the inventory item is not allowed.
    OverSize = 9,

    /// Indicates that the transaction has expired.
    Expired = 10,

    /// Indicates that the transaction failed to verify due to insufficient funds.
    InsufficientFunds = 11,

    /// Indicates that the transaction failed to verify because it didn't comply with the policy.
    PolicyFail = 12,

    /// Indicates that the transaction failed to verify because it conflicts with on-chain or mempooled transactions.
    HasConflicts = 13,

    /// Indicates that the inventory item failed to verify due to other reasons.
    Unknown = 14,
}

impl From<VerifyResult> for u8 {
    fn from(result: VerifyResult) -> Self {
        result as u8
    }
}

impl TryFrom<u8> for VerifyResult {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(VerifyResult::Succeed),
            1 => Ok(VerifyResult::AlreadyExists),
            2 => Ok(VerifyResult::AlreadyInPool),
            3 => Ok(VerifyResult::OutOfMemory),
            4 => Ok(VerifyResult::UnableToVerify),
            5 => Ok(VerifyResult::Invalid),
            6 => Ok(VerifyResult::InvalidScript),
            7 => Ok(VerifyResult::InvalidAttribute),
            8 => Ok(VerifyResult::InvalidSignature),
            9 => Ok(VerifyResult::OverSize),
            10 => Ok(VerifyResult::Expired),
            11 => Ok(VerifyResult::InsufficientFunds),
            12 => Ok(VerifyResult::PolicyFail),
            13 => Ok(VerifyResult::HasConflicts),
            14 => Ok(VerifyResult::Unknown),
            _ => Err(()),
        }
    }
}
