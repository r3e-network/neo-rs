// Copyright (C) 2015-2025 The Neo Project.
//
// not_valid_before.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_io::{impl_serializable, BinaryWriter, IoResult, Serializable};
use neo_data_cache::DataCache;
use neo_config::ProtocolSettings;
use serde::{Deserialize, Serialize};

/// Represents a not-valid-before transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotValidBefore {
    /// Indicates that the transaction is not valid before this height.
    pub height: u32,
}

impl NotValidBefore {
    /// Creates a new not-valid-before attribute.
    pub fn new(height: u32) -> Self {
        Self { height }
    }

    /// Verify the not-valid-before attribute.


    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

impl_serializable! {
    struct NotValidBefore {
        height: u32,
    }
}
