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

/// Peers information matching C# RpcPeers
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
    /// Matches C# ToJson
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
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let unconnected = json
            .get("unconnected")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| RpcPeer::from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();

        let bad = json
            .get("bad")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| RpcPeer::from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();

        let connected = json
            .get("connected")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| item.as_ref())
                    .filter_map(|token| token.as_object())
                    .filter_map(|obj| RpcPeer::from_json(obj).ok())
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            unconnected,
            bad,
            connected,
        })
    }
}

/// Individual peer information matching C# RpcPeer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcPeer {
    /// Peer address
    pub address: String,

    /// Peer port
    pub port: i32,
}

impl RpcPeer {
    /// Converts to JSON
    /// Matches C# ToJson
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("address".to_string(), JToken::String(self.address.clone()));
        json.insert("port".to_string(), JToken::Number(self.port as f64));
        json
    }

    /// Creates from JSON
    /// Matches C# FromJson
    pub fn from_json(json: &JObject) -> Result<Self, String> {
        let address = json
            .get("address")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'address' field")?
            .to_string();

        let port_str = json
            .get("port")
            .and_then(|v| v.as_string())
            .ok_or("Missing or invalid 'port' field")?;
        let port = port_str
            .parse::<i32>()
            .map_err(|_| format!("Invalid port value: {}", port_str))?;

        Ok(Self { address, port })
    }
}
