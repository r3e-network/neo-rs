// Copyright (C) 2015-2024 The Neo Project.
//
// named_curve_hash.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_sdk::prelude::*;

/// Represents a pair of the named curve used in ECDSA and a hash algorithm used to hash message.
#[repr(u8)]
pub enum NamedCurveHash {
    /// The secp256k1 curve and SHA256 hash algorithm.
    Secp256k1SHA256 = 22,

    /// The secp256r1 curve, which known as prime256v1 or nistP-256, and SHA256 hash algorithm.
    Secp256r1SHA256 = 23,

    /// The secp256k1 curve and Keccak256 hash algorithm.
    Secp256k1Keccak256 = 122,

    /// The secp256r1 curve, which known as prime256v1 or nistP-256, and Keccak256 hash algorithm.
    Secp256r1Keccak256 = 123,
}

impl NamedCurveHash {
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            22 => Some(Self::Secp256k1SHA256),
            23 => Some(Self::Secp256r1SHA256),
            122 => Some(Self::Secp256k1Keccak256),
            123 => Some(Self::Secp256r1Keccak256),
            _ => None,
        }
    }
}
