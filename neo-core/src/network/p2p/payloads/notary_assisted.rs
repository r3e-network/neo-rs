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

use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::Helper;
use crate::UInt160;
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
    fn get_notary_hash() -> UInt160 {
        Helper::get_contract_hash(&UInt160::zero(), 0, "Notary")
    }

    /// Verify the notary-assisted attribute.
    pub fn verify(
        &self,
        _settings: &ProtocolSettings,
        _snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool {
        let notary_hash = Self::get_notary_hash();

        if tx.sender() == Some(notary_hash) {
            // Payer is in the second position
            return tx.signers().len() == 2;
        }

        tx.signers().iter().any(|s| s.account == notary_hash)
    }

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
        writer.write_u8(self.nkeys)
    }
}

impl Serializable for NotaryAssisted {
    fn size(&self) -> usize {
        1 // u8
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.nkeys)
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let nkeys = reader.read_u8()?;
        Ok(Self { nkeys })
    }
}
