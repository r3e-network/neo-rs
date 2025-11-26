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
use crate::smart_contract::helper::Helper;
use crate::{persistence::DataCache, protocol_settings::ProtocolSettings, UInt160};

/// Represents an object that can be verified in the NEO network.
pub trait IVerifiable: Serializable {
    /// Gets the script hashes that should be verified for this IVerifiable object.
    fn get_script_hashes_for_verifying(&self, snapshot: &DataCache) -> Vec<UInt160>;

    /// Gets the witnesses of the IVerifiable object.
    fn get_witnesses(&self) -> Vec<&Witness>;

    /// Gets mutable witnesses of the IVerifiable object.
    fn get_witnesses_mut(&mut self) -> Vec<&mut Witness>;

    /// Verify witnesses with a gas limit.
    /// Matches C# IVerifiable.VerifyWitnesses extension method.
    ///
    /// This method verifies all witnesses by:
    /// 1. Getting script hashes to verify
    /// 2. Getting witnesses
    /// 3. Executing verification scripts with gas limit
    /// 4. Returning true if all verifications pass
    fn verify_witnesses(
        &self,
        settings: &ProtocolSettings,
        snapshot: &DataCache,
        max_gas: i64,
    ) -> bool
    where
        Self: Sized,
    {
        Helper::verify_witnesses(self, settings, snapshot, max_gas)
    }
}
