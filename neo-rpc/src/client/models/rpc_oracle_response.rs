// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_oracle_response.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Lightweight representation of an oracle response entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RpcOracleResponse {
    /// Oracle request hash or identifier.
    pub id: u64,
    /// Oracle response code.
    pub code: i32,
    /// Result payload encoded as base64.
    pub result: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_response_roundtrip() {
        let resp = RpcOracleResponse {
            id: 42,
            code: 0x16,
            result: "aGVsbG8=".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: RpcOracleResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, resp.id);
        assert_eq!(parsed.code, resp.code);
        assert_eq!(parsed.result, resp.result);
    }
}
