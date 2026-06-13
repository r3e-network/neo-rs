// Copyright (C) 2015-2025 The Neo Project.
//
// conflicts.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_io::{BinaryWriter, IoResult, Serializable, impl_serializable};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Represents a conflicts transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflicts {
    /// Indicates the conflict transaction hash.
    pub hash: UInt256,
}

impl Conflicts {
    /// Creates a new conflicts attribute.
    pub fn new(hash: UInt256) -> Self {
        Self { hash }
    }

    /// Verify the conflicts attribute.

    /// Calculate network fee for this attribute.
    pub fn calculate_network_fee(
        &self,
        base_fee: i64,
        tx: &super::transaction::Transaction,
    ) -> i64 {
        tx.signers().len() as i64 * base_fee
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

impl_serializable! {
    struct Conflicts {
        hash: UInt256,
    }
}
