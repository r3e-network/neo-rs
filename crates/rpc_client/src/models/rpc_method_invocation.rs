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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rpc_method_invocation_defaults_parameters() {
        let invocation = RpcMethodInvocation {
            script: "00c56b".into(),
            parameters: Vec::new(),
        };
        let json = serde_json::to_string(&invocation).expect("serialize");
        let parsed: RpcMethodInvocation = serde_json::from_str(&json).expect("deserialize");
        assert!(parsed.parameters.is_empty());
        assert_eq!(parsed.script, "00c56b");
    }

    #[test]
    fn rpc_method_invocation_parses_parameters() {
        let json =
            r#"{ "script": "00c56b", "parameters": [ { "type": "String", "value": "hello" } ] }"#;
        let parsed: RpcMethodInvocation = serde_json::from_str(json).expect("deserialize");
        assert_eq!(parsed.parameters.len(), 1);
    }
}
