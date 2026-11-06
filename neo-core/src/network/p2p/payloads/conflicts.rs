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

//! Conflicts transaction attribute.
//!
//! The canonical `Conflicts` payload is owned by `neo-protocol` and re-exported
//! here for backward compatibility (R2 leaf-type unification). All structural
//! API (`new`, the `hash` field, and wire `Serializable`) is provided by
//! `neo-protocol`.
//!
//! neo-core keeps only the snapshot-aware verification and fee helpers, which
//! depend on the native ledger contract. Because `Conflicts` is now a foreign
//! type, these cannot be inherent methods (orphan rule) and are exposed via the
//! [`ConflictsExt`] extension trait instead.

use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use tracing::warn;

/// Canonical conflicts transaction attribute, owned by `neo-protocol`.
pub use neo_protocol::payloads::conflicts::Conflicts;

/// neo-core-specific verification and fee helpers for [`Conflicts`].
pub trait ConflictsExt {
    /// Verify the conflicts attribute against the ledger.
    /// Matches C# Verify method.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool;

    /// Calculate network fee for this attribute.
    fn calculate_network_fee(&self, base_fee: i64, tx: &super::transaction::Transaction) -> i64;
}

impl ConflictsExt for Conflicts {
    fn verify(
        &self,
        _settings: &ProtocolSettings,
        snapshot: &DataCache,
        _tx: &super::transaction::Transaction,
    ) -> bool {
        let ledger = LedgerContract::new();
        match ledger.contains_transaction(snapshot, &self.hash) {
            Ok(exists) => !exists,
            Err(err) => {
                warn!(target: "neo", hash = %self.hash, error = %err, "failed to verify conflicts attribute against ledger");
                false
            }
        }
    }

    fn calculate_network_fee(&self, base_fee: i64, tx: &super::transaction::Transaction) -> i64 {
        tx.signers().len() as i64 * base_fee
    }
}
