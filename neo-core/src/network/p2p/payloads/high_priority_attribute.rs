// Copyright (C) 2015-2025 The Neo Project.
//
// high_priority_attribute.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! High priority transaction attribute.
//!
//! The canonical `HighPriorityAttribute` payload is owned by `neo-protocol` and
//! re-exported here for backward compatibility (R2 leaf-type unification). All
//! structural API (`new`, `Default`, and wire `Serializable`) is provided by
//! `neo-protocol`.
//!
//! neo-core keeps only the snapshot-aware verification helper, exposed via the
//! [`HighPriorityAttributeExt`] extension trait because it cannot be an
//! inherent method on a foreign type (orphan rule).

use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::helpers::NativeHelpers;

/// Canonical high priority transaction attribute, owned by `neo-protocol`.
pub use neo_protocol::payloads::high_priority_attribute::HighPriorityAttribute;

/// neo-core-specific verification helper for [`HighPriorityAttribute`].
pub trait HighPriorityAttributeExt {
    /// Verify the high priority attribute.
    /// Matches C# Verify method.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool;
}

impl HighPriorityAttributeExt for HighPriorityAttribute {
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        tx: &super::transaction::Transaction,
    ) -> bool {
        if settings.standby_committee.is_empty() {
            return false;
        }

        let address = NativeHelpers::committee_address(settings, Some(snapshot));
        tx.signers().iter().any(|signer| signer.account == address)
    }
}
