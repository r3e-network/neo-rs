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

//! Notary-assisted transaction attribute.
//!
//! The canonical `NotaryAssisted` payload is owned by `neo-protocol` and
//! re-exported here for backward compatibility (R2 leaf-type unification). All
//! structural API (`new`, the `nkeys` field, and wire `Serializable`) is
//! provided by `neo-protocol`.
//!
//! neo-core keeps only the snapshot-aware verification and fee helpers, exposed
//! via the [`NotaryAssistedExt`] extension trait because they cannot be
//! inherent methods on a foreign type (orphan rule).

use crate::UInt160;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::Helper;

/// Canonical notary-assisted transaction attribute, owned by `neo-protocol`.
pub use neo_protocol::payloads::notary_assisted::NotaryAssisted;

/// Get the notary contract hash.
fn get_notary_hash() -> UInt160 {
    Helper::get_contract_hash(&UInt160::zero(), 0, "Notary")
}

/// neo-core-specific verification and fee helpers for [`NotaryAssisted`].
pub trait NotaryAssistedExt {
    /// Verify the notary-assisted attribute.
    /// Matches C# Verify method.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool;

    /// Calculate network fee for this attribute.
    ///
    /// Network fee consists of the base Notary service fee per key multiplied by
    /// the expected number of transactions that should be collected by the
    /// service to complete Notary request increased by one (for Notary node
    /// witness itself).
    fn calculate_network_fee(&self, base_fee: i64, tx: &super::transaction::Transaction) -> i64;
}

impl NotaryAssistedExt for NotaryAssisted {
    fn verify(
        &self,
        _settings: &ProtocolSettings,
        _snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool {
        let notary_hash = get_notary_hash();

        if tx.sender() == Some(notary_hash) {
            // Payer is in the second position
            return tx.signers().len() == 2;
        }

        tx.signers().iter().any(|s| s.account == notary_hash)
    }

    fn calculate_network_fee(&self, base_fee: i64, _tx: &super::transaction::Transaction) -> i64 {
        (self.nkeys as i64 + 1) * base_fee
    }
}
