// Copyright (C) 2015-2025 The Neo Project.
//
// transaction_attribute.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Transaction attribute.
//!
//! The canonical `TransactionAttribute` enum is owned by `neo-protocol` and
//! re-exported here for backward compatibility (R2 leaf-type unification). All
//! structural API (the variants, `type_id`, `attribute_type`, `allow_multiple`,
//! the convenience constructors, `to_json`, `deserialize_from`, and wire
//! `Serializable`) is provided by `neo-protocol`.
//!
//! neo-core keeps only the snapshot-aware verification and fee helpers, which
//! depend on the native policy/ledger contracts and the per-variant attribute
//! verification. Because `TransactionAttribute` is now a foreign type, these
//! cannot be inherent methods (orphan rule) and are exposed via the
//! [`TransactionAttributeExt`] extension trait instead. The per-variant
//! verification/fee logic lives in the subtype extension traits
//! ([`ConflictsExt`], [`NotaryAssistedExt`], [`NotValidBeforeExt`],
//! [`OracleResponseExt`], [`HighPriorityAttributeExt`]).

use super::{
    HighPriorityAttribute, conflicts::ConflictsExt,
    high_priority_attribute::HighPriorityAttributeExt, not_valid_before::NotValidBeforeExt,
    notary_assisted::NotaryAssistedExt, oracle_response::OracleResponseExt,
    transaction::Transaction,
};
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::PolicyContract;

/// Canonical transaction attribute enum, owned by `neo-protocol`.
pub use neo_protocol::payloads::transaction_attribute::TransactionAttribute;

/// neo-core-specific verification and fee helpers for [`TransactionAttribute`].
pub trait TransactionAttributeExt {
    /// Verify the attribute.
    /// Matches C# Verify method.
    fn verify(&self, settings: &ProtocolSettings, snapshot: &DataCache, tx: &Transaction) -> bool;

    /// Calculate the network fee for this attribute.
    /// Matches C# CalculateNetworkFee method.
    fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64;
}

impl TransactionAttributeExt for TransactionAttribute {
    fn verify(&self, settings: &ProtocolSettings, snapshot: &DataCache, tx: &Transaction) -> bool {
        match self {
            Self::HighPriority => HighPriorityAttribute.verify(settings, snapshot, tx),
            Self::OracleResponse(attr) => attr.verify(settings, snapshot, tx),
            Self::NotValidBefore(attr) => attr.verify(settings, snapshot, tx),
            Self::Conflicts(attr) => attr.verify(settings, snapshot, tx),
            Self::NotaryAssisted(attr) => attr.verify(settings, snapshot, tx),
        }
    }

    fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64 {
        let policy = PolicyContract::new();
        let base_fee = policy
            .get_attribute_fee_snapshot(snapshot, self.type_id())
            .unwrap_or(PolicyContract::DEFAULT_ATTRIBUTE_FEE as i64);

        match self {
            Self::Conflicts(attr) => attr.calculate_network_fee(base_fee, tx),
            Self::NotaryAssisted(attr) => attr.calculate_network_fee(base_fee, tx),
            _ => base_fee,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UInt256;
    use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
    use crate::network::p2p::payloads::{
        Conflicts, NotValidBefore, NotaryAssisted, OracleResponse, OracleResponseCode,
    };

    fn sample_attributes() -> Vec<TransactionAttribute> {
        vec![
            TransactionAttribute::HighPriority,
            TransactionAttribute::OracleResponse(OracleResponse::new(
                7,
                OracleResponseCode::Success,
                vec![1, 2, 3],
            )),
            TransactionAttribute::NotValidBefore(NotValidBefore::new(42)),
            TransactionAttribute::Conflicts(Conflicts::new(UInt256::from([0xAA; 32]))),
            TransactionAttribute::NotaryAssisted(NotaryAssisted::new(2)),
        ]
    }

    #[test]
    fn wire_mapping_preserves_attribute_type_bytes() {
        for attribute in sample_attributes() {
            let expected_type = attribute.type_id();
            let mut writer = BinaryWriter::new();

            Serializable::serialize(&attribute, &mut writer).unwrap();
            let bytes = writer.into_bytes();

            assert_eq!(bytes[0], expected_type.to_byte(), "{attribute:?}");

            let mut reader = MemoryReader::new(&bytes);
            let decoded = TransactionAttribute::deserialize_from(&mut reader).unwrap();

            assert_eq!(decoded.type_id(), expected_type);
            assert_eq!(reader.remaining(), 0);
        }
    }

    #[test]
    fn multiplicity_matches_attribute_type_table() {
        for attribute in sample_attributes() {
            assert_eq!(
                attribute.allow_multiple(),
                attribute.type_id().allows_multiple(),
                "{attribute:?}"
            );
        }

        assert!(TransactionAttribute::Conflicts(Conflicts::new(UInt256::zero())).allow_multiple());
        assert!(!TransactionAttribute::NotaryAssisted(NotaryAssisted::new(1)).allow_multiple());
    }
}
