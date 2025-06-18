// Copyright (C) 2015-2025 The Neo Project.
//
// uint160_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Extension traits for UInt160.

use crate::uint160::UInt160;

/// Extension trait for UInt160.
pub trait UInt160Extensions {
    /// Converts the UInt160 to a byte array.
    ///
    /// # Returns
    ///
    /// A byte array representation of the UInt160.
    fn to_array(&self) -> [u8; 20];
}

// UInt160 now has a to_array method directly, so this is just a pass-through
impl UInt160Extensions for UInt160 {
    fn to_array(&self) -> [u8; 20] {
        self.to_array()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_array() {
        let mut uint = UInt160::new();
        uint.value1 = 1;

        let array = uint.to_array();
        assert_eq!(array[0], 1);
        for i in 1..20 {
            assert_eq!(array[i], 0);
        }
    }
}
