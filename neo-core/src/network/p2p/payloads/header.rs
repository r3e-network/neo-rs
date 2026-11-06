// Copyright (C) 2015-2025 The Neo Project.
//
// header.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! P2P block header — re-exported from neo-protocol with neo-core extension
//! traits for verification and witness handling.

pub use neo_protocol::payloads::header::Header;

use super::witness::Witness;
use crate::persistence::DataCache;
use crate::smart_contract::native::LedgerContract;
use crate::{UInt160, UInt256};

mod verification;
pub use verification::HeaderExt;

#[cfg(test)]
mod tests;

// ─── Local trait impls (neo-core specific) ──────────────────────────────────

impl crate::VerifiableExt for Header {
    fn script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160> {
        if *self.prev_hash() == UInt256::default() {
            return vec![self.witness.script_hash()];
        }

        let ledger = LedgerContract::new();
        let prev = match ledger.get_trimmed_block(snapshot, self.prev_hash()) {
            Ok(Some(prev)) => prev,
            Ok(None) => {
                tracing::warn!(
                    prev_hash = %self.prev_hash(),
                    "previous header not found when verifying header"
                );
                return Vec::new();
            }
            Err(err) => {
                tracing::warn!(
                    prev_hash = %self.prev_hash(),
                    error = %err,
                    "failed to fetch previous header when verifying header"
                );
                return Vec::new();
            }
        };

        vec![prev.header.next_consensus]
    }
}

impl crate::WitnessData for Header {
    fn witnesses(&self) -> Vec<&Witness> {
        <Header as neo_protocol::witness::WitnessData>::witnesses(self)
    }

    fn witnesses_mut(&mut self) -> Vec<&mut Witness> {
        <Header as neo_protocol::witness::WitnessData>::witnesses_mut(self)
    }
}
