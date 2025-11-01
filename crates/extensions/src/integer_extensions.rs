// Copyright (C) 2015-2025 The Neo Project.
//
// integer_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Integer extensions matching C# IntegerExtensions exactly
pub trait IntegerExtensions {
    /// Gets the size of variable-length of the data.
    /// Matches C# GetVarSize method
    fn get_var_size(&self) -> u8;
}

impl IntegerExtensions for i32 {
    fn get_var_size(&self) -> u8 {
        (*self as i64).get_var_size()
    }
}

impl IntegerExtensions for u16 {
    fn get_var_size(&self) -> u8 {
        (*self as i64).get_var_size()
    }
}

impl IntegerExtensions for u32 {
    fn get_var_size(&self) -> u8 {
        (*self as i64).get_var_size()
    }
}

impl IntegerExtensions for i64 {
    fn get_var_size(&self) -> u8 {
        if *self < 0xFD {
            1
        } else if *self <= u16::MAX as i64 {
            3
        } else if *self <= u32::MAX as i64 {
            5
        } else {
            9
        }
    }
}

impl IntegerExtensions for usize {
    fn get_var_size(&self) -> u8 {
        (*self as i64).get_var_size()
    }
}
