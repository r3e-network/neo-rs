// Copyright (C) 2015-2025 The Neo Project.
//
// witness_scope.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    /// Represents the scope of a Witness.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct WitnessScope: u8 {
        /// Indicates that no contract was witnessed. Only sign the transaction.
        const NONE = 0x00;

        /// Indicates that the calling contract must be the entry contract.
        /// The witness/permission/signature given on first invocation will automatically expire if entering deeper internal invokes.
        /// This can be the default safe choice for native NEO/GAS (previously used on Neo 2 as "attach" mode).
        const CALLED_BY_ENTRY = 0x01;

        /// Custom hash for contract-specific.
        const CUSTOM_CONTRACTS = 0x10;

        /// Custom pubkey for group members.
        const CUSTOM_GROUPS = 0x20;

        /// Indicates that the current context must satisfy the specified rules.
        const WITNESS_RULES = 0x40;

        /// This allows the witness in all contexts (default Neo2 behavior).
        /// Note: It cannot be combined with other flags.
        const GLOBAL = 0x80;
    }
}
