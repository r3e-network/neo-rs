// Copyright (C) 2015-2025 The Neo Project.
//
// keys.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Storage keys for state service.
/// Matches C# Keys class exactly
pub struct Keys;

impl Keys {
    /// Creates a state root key for the given index.
    /// Matches C# StateRoot method
    pub fn state_root(index: u32) -> Vec<u8> {
        let mut buffer = vec![0u8; 5]; // sizeof(uint) + 1
        buffer[0] = 1;
        buffer[1..5].copy_from_slice(&index.to_be_bytes());
        buffer
    }
    
    /// Current local root index key.
    /// Matches C# CurrentLocalRootIndex field
    pub const CURRENT_LOCAL_ROOT_INDEX: &'static [u8] = &[0x02];
    
    /// Current validated root index key.
    /// Matches C# CurrentValidatedRootIndex field
    pub const CURRENT_VALIDATED_ROOT_INDEX: &'static [u8] = &[0x04];
}