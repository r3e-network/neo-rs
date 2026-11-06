// Copyright (C) 2015-2025 The Neo Project.
//
// extensible_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Extensible relay payload (D5-1 dedup gate).
//!
//! The canonical type is owned by `neo-protocol`
//! (`neo_protocol::payloads::extensible::Extensible`) and re-exported here
//! under its historical `ExtensiblePayload` name, unifying the `TypeId` with
//! every other consumer of the payload. The structural surface (public
//! fields, wire codec, hash caching, `Default`, and the
//! `Serializable` / `Verifiable` / `Inventory` / `SerializablePayload` /
//! `WitnessData` (protocol) impls) lives in `neo-protocol`.
//!
//! This module keeps only the neo-core-specific extension surface:
//!
//! - [`ExtensiblePayloadExt::verify`]: snapshot-driven verification (current
//!   height, witness whitelist, VM witness checks) which depends on the
//!   smart contract engine and cannot live in the protocol leaf. It is an
//!   extension trait because inherent impls cannot be attached to the
//!   re-exported foreign type.
//! - [`crate::VerifiableExt`] and [`crate::WitnessData`] impls: local traits
//!   implemented for the re-exported type (orphan-rule safe because the
//!   traits are defined in neo-core).

use crate::UInt160;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use std::collections::HashSet;

/// Canonical extensible relay payload, owned by `neo-protocol`.
///
/// Transitional alias: `ExtensiblePayload` is `neo-protocol`'s `Extensible`.
pub use neo_protocol::payloads::extensible::Extensible as ExtensiblePayload;

/// Snapshot-driven verification for [`ExtensiblePayload`] that requires the
/// native-contract and VM layers and therefore stays in neo-core.
pub trait ExtensiblePayloadExt {
    /// Verify the payload against protocol settings, the current snapshot,
    /// and the extensible witness whitelist.
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        extensible_witness_white_list: &HashSet<UInt160>,
    ) -> bool;
}

impl ExtensiblePayloadExt for ExtensiblePayload {
    fn verify(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        extensible_witness_white_list: &HashSet<UInt160>,
    ) -> bool {
        let height = LedgerContract::new().current_index(snapshot).unwrap_or(0);

        // Check if within valid block range
        if height < self.valid_block_start || height >= self.valid_block_end {
            return false;
        }

        // Check if sender is in whitelist
        if !extensible_witness_white_list.contains(&self.sender) {
            return false;
        }

        // Verify witness with max gas of 0.06 GAS
        crate::smart_contract::helper::Helper::verify_witnesses(self, settings, snapshot, 6_000_000)
    }
}

impl crate::VerifiableExt for ExtensiblePayload {
    fn script_hashes_for_verifying(&self, _snapshot: &DataCache) -> Vec<UInt160> {
        vec![self.sender]
    }
}

impl crate::WitnessData for ExtensiblePayload {
    fn witnesses(&self) -> Vec<&crate::witness::Witness> {
        vec![&self.witness]
    }

    fn witnesses_mut(&mut self) -> Vec<&mut crate::witness::Witness> {
        vec![&mut self.witness]
    }
}
