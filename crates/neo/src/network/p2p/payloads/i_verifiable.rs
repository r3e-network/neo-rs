// Copyright (C) 2015-2025 The Neo Project.
//
// i_verifiable.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::witness::Witness;
use crate::neo_io::Serializable;
use crate::{neo_system::ProtocolSettings, persistence::DataCache, UInt160, UInt256};

/// Represents an object that can be verified in the NEO network.
pub trait IVerifiable: Serializable {
    /// Gets the script hashes that should be verified for this IVerifiable object.
    fn get_script_hashes_for_verifying(&self, snapshot: &dyn DataCache) -> Vec<UInt160>;

    /// Gets the witnesses of the IVerifiable object.
    fn get_witnesses(&self) -> Vec<&Witness>;

    /// Gets mutable witnesses of the IVerifiable object.
    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness>;

    /// Verify witnesses with a gas limit.
    fn verify_witnesses(
        &self,
        settings: &ProtocolSettings,
        snapshot: &dyn DataCache,
        max_gas: i64,
    ) -> bool {
        // This would require VM execution to verify scripts
        // For now, return a placeholder
        // In full implementation, this would:
        // 1. Get script hashes for verifying
        // 2. Get witnesses
        // 3. Execute verification scripts with gas limit
        // 4. Return true if all verifications pass
        true
    }
}
