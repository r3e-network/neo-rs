// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_peers.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_json::{JArray, JObject, JToken};
use serde::{Deserialize, Serialize};

/// Peers information matching C# `RpcPeers`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPeers {
    /// Unconnected peers
    pub unconnected: Vec<RpcPeer>,

    /// Bad peers
    pub bad: Vec<RpcPeer>,

    /// Connected peers
    pub connected: Vec<RpcPeer>,
}

impl RpcPeers {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();

        let unconnected_array: Vec<JToken> = self
            .unconnected
            .iter()
            .map(|p| JToken::Object(p.to_json()))
            .collect();
        json.insert(
            "unconnected".to_string(),
            JToken::Array(JArray::from(unconnected_array)),
        );

        let bad_array: Vec<JToken> = self
            .bad
            .iter()
            .map(|p| JToken::Object(p.to_json()))
            .collect();
        json.insert("bad".to_string(), JToken::Array(JArray::from(bad_array)));

        let connected_array: Vec<JToken> = self
            .connected
            .iter()
            .map(|p| JToken::Object(p.to_json()))
            .collect();
        json.insert(
            "connected".to_string(),
            JToken::Array(JArray::from(connected_array)),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let unconnected = parse_peer_list(json, "unconnected")?;
        let bad = parse_peer_list(json, "bad")?;
        let connected = parse_peer_list(json, "connected")?;

        Ok(Self {
            unconnected,
            bad,
            connected,
        })
    }
}

/// Individual peer information matching C# `RpcPeer`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPeer {
    /// Peer address
    pub address: String,

    /// Peer port
    pub port: i32,
}

impl RpcPeer {
    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("port".to_string(), JToken::Number(f64::from(self.port)));
        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let address = json
            .get("address")
            .and_then(neo_json::JToken::as_string)
            .ok_or("Missing or invalid 'address' field")?;

        let port_token = json.get("port").ok_or("Missing or invalid 'port' field")?;
        let port = if let Some(number) = port_token.as_number() {
            number as i32
        } else if let Some(text) = port_token.as_string() {
            text.parse::<i32>()
                .map_err(|_| format!("Invalid port value: {text}"))?
        } else {
            return Err("Invalid 'port' field type".to_string());
        };

        Ok(Self { address, port })
    }
}

fn parse_peer_list(json: &JObject, field: &str) -> Result<Vec<RpcPeer>, String> {
    let peers = json
        .get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_ref())
                .map(|token| {
                    token
                        .as_object()
                        .ok_or_else(|| format!("{field} entry must be an object"))
                        .and_then(RpcPeer::from_json)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .unwrap_or_else(|| Ok(Vec::new()))?;

    Ok(peers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_json::JToken;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn rpc_peer_roundtrip_accepts_numeric_port() {
        let peer = RpcPeer {
            address: "127.0.0.1".to_string(),
            port: 20333,
        };
        let json = peer.to_json();
        assert_eq!(json.get("port").and_then(|v| v.as_number()), Some(20333.0));

        let parsed = RpcPeer::from_json(&json).expect("peer");
        assert_eq!(parsed.address, peer.address);
        assert_eq!(parsed.port, peer.port);
    }

    #[test]
    fn rpc_peer_accepts_string_port() {
        let mut json = JObject::new();
        json.insert(
            "address".to_string(),
            JToken::String("10.0.0.1".to_string()),
        );
        json.insert("port".to_string(), JToken::String("20334".to_string()));

        let parsed = RpcPeer::from_json(&json).expect("peer");
        assert_eq!(parsed.address, "10.0.0.1");
        assert_eq!(parsed.port, 20334);
    }

    #[test]
    fn rpc_peers_roundtrip() {
        let peers = RpcPeers {
            unconnected: vec![RpcPeer {
                address: "1.1.1.1".into(),
                port: 1,
            }],
            bad: vec![],
            connected: vec![RpcPeer {
                address: "2.2.2.2".into(),
                port: 2,
            }],
        };

        let json = peers.to_json();
        let parsed = RpcPeers::from_json(&json).expect("peers");

        assert_eq!(parsed.unconnected.len(), 1);
        assert_eq!(parsed.connected.len(), 1);
        assert_eq!(parsed.unconnected[0].address, "1.1.1.1");
        assert_eq!(parsed.connected[0].port, 2);
    }

    #[test]
    fn parse_peer_list_fails_on_bad_entry_type() {
        let mut json = JObject::new();
        json.insert("connected".to_string(), JToken::Boolean(true));
        let parsed = RpcPeers::from_json(&json);
        assert!(parsed.is_ok(), "non-array defaults to empty peers list");

        let mut invalid = JObject::new();
        invalid.insert(
            "connected".to_string(),
            JToken::Array(JArray::from(vec![JToken::Null])),
        );
        let parsed = RpcPeers::from_json(&invalid);
        assert!(parsed.is_err());
    }

    fn load_rpc_case_result(name: &str) -> Option<JObject> {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
        if !path.exists() {
            eprintln!(
                "SKIP: neo_csharp submodule not initialized ({})",
                path.display()
            );
            return None;
        }
        let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
        let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
        let cases = token
            .as_array()
            .expect("RpcTestCases.json should be an array");
        for entry in cases.children() {
            let token = entry.as_ref().expect("array entry");
            let obj = token.as_object().expect("case object");
            let case_name = obj
                .get("Name")
                .and_then(|value| value.as_string())
                .unwrap_or_default();
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                return Some(result.clone());
            }
        }
        eprintln!("SKIP: RpcTestCases.json missing case: {name}");
        None
    }

    #[test]
    fn peers_to_json_matches_rpc_test_case() {
        let Some(expected) = load_rpc_case_result("getpeersasync") else {
            return;
        };
        let parsed = RpcPeers::from_json(&expected).expect("parse");
        let actual = parsed.to_json();
        assert_eq!(expected.to_string(), actual.to_string());
    }
}
