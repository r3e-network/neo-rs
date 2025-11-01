// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_exception.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::fmt;
use thiserror::Error;

/// RPC exception matching C# RpcException
#[derive(Error, Debug, Clone)]
pub struct RpcException {
    /// Error code (HResult in C#)
    pub code: i32,

    /// Error message
    pub message: String,
}

impl RpcException {
    /// Creates a new RPC exception
    /// Matches C# constructor
    pub fn new(code: i32, message: String) -> Self {
        Self { code, message }
    }
}

impl fmt::Display for RpcException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}
