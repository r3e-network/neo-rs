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

//! Not-valid-before transaction attribute.
//!
//! The canonical `NotValidBefore` payload is owned by `neo-protocol` and
//! re-exported here for backward compatibility (R2 leaf-type unification). All
//! structural API (`new`, the `height` field, and wire `Serializable`) is
//! provided by `neo-protocol`.
//!
//! neo-core keeps only the snapshot-aware verification helper, exposed via the
//! [`NotValidBeforeExt`] extension trait because it cannot be an inherent
//! method on a foreign type (orphan rule).

use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;

/// Canonical not-valid-before transaction attribute, owned by `neo-protocol`.
pub use neo_protocol::payloads::not_valid_before::NotValidBefore;

/// neo-core-specific verification helper for [`NotValidBefore`].
pub trait NotValidBeforeExt {
    /// Verify the not-valid-before attribute.
    /// Matches C# Verify method.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool;
}

impl NotValidBeforeExt for NotValidBefore {
    fn verify(
        &self,
        _settings: &ProtocolSettings,
        snapshot: &DataCache,
        _tx: &super::transaction::Transaction,
    ) -> bool {
        let current_height = LedgerContract::new().current_index(snapshot).unwrap_or(0);
        current_height >= self.height
    }
}
