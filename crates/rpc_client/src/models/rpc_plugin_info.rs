// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_plugin_info.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Metadata describing an RPC plugin exposed by the node.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcPluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version string.
    pub version: String,
    /// Optional description.
    #[serde(default)]
    pub description: Option<String>,
}
