// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_mempool_accepted.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Model representing the hashes accepted into the mempool when invoking
/// the `getrawmempool` RPC with `true` flag. Mirrors the shape exposed by
/// the C# client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcMempoolAccepted {
    /// Transaction hashes currently accepted in the mempool.
    pub hashes: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mempool_accepted_roundtrip() {
        let model = RpcMempoolAccepted {
            hashes: vec!["0x01".to_string(), "0x02".to_string()],
        };
        let json = serde_json::to_string(&model).unwrap();
        let parsed: RpcMempoolAccepted = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.hashes, model.hashes);
    }
}
