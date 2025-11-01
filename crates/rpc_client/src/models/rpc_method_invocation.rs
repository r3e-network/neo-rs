// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_method_invocation.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Captures the payload of a method invocation submitted via RPC.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcMethodInvocation {
    /// The contract script being executed.
    pub script: String,
    /// Optional parameters supplied to the invocation.
    #[serde(default)]
    pub parameters: Vec<Value>,
}
