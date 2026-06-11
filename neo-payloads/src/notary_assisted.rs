// Copyright (C) 2015-2025 The Neo Project.
//
// notary_assisted.rs file belongs to the neo project and is free
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
use neo_primitives::UInt160;
use serde::{Deserialize, Serialize};

/// Represents a notary-assisted transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotaryAssisted {
    /// Indicates the number of keys participating in the transaction (main or fallback) signing process.
    pub nkeys: u8,
}

impl NotaryAssisted {
    /// Creates a new notary-assisted attribute.
    pub fn new(nkeys: u8) -> Self {
        Self { nkeys }
    }

    /// Get the notary contract hash.
    fn get_notary_hash() -> UInt160 { UInt160::zero() }

    /// Verify the notary-assisted attribute.


    /// Calculate network fee for this attribute.
    /// Network fee consists of the base Notary service fee per key multiplied by the expected
    /// number of transactions that should be collected by the service to complete Notary request
    /// increased by one (for Notary node witness itself).
    pub fn calculate_network_fee(
        &self,
        base_fee: i64,
        _tx: &super::transaction::Transaction,
    ) -> i64 {
        (self.nkeys as i64 + 1) * base_fee
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

impl_serializable! {
    struct NotaryAssisted {
        nkeys: u8,
    }
}
