// Copyright (C) 2015-2024 The Neo Project.
//
// Role.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_sdk::prelude::*;

/// Represents the roles in the NEO system.
#[repr(u8)]
pub enum Role {
    /// The validators of state. Used to generate and sign the state root.
    StateValidator = 4,

    /// The nodes used to process Oracle requests.
    Oracle = 8,

    /// NeoFS Alphabet nodes.
    NeoFSAlphabetNode = 16,

    /// P2P Notary nodes used to process P2P notary requests.
    P2PNotary = 32,
}

impl From<u8> for Role {
    fn from(value: u8) -> Self {
        match value {
            4 => Role::StateValidator,
            8 => Role::Oracle,
            16 => Role::NeoFSAlphabetNode,
            32 => Role::P2PNotary,
            _ => panic!("Invalid Role value"),
        }
    }
}

impl Into<u8> for Role {
    fn into(self) -> u8 {
        self as u8
    }
}
