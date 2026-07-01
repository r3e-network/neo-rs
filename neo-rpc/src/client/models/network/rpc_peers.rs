use super::super::utility::{
    object_array, parse_number_or_string_token, parse_optional_present_token_array_strict,
};
use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
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

        json.insert(
            "unconnected".to_string(),
            object_array(&self.unconnected, RpcPeer::to_json),
        );

        json.insert("bad".to_string(), object_array(&self.bad, RpcPeer::to_json));

        json.insert(
            "connected".to_string(),
            object_array(&self.connected, RpcPeer::to_json),
        );

        json
    }

    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
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
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let address = json
            .get("address")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'address' field"))?;

        let port_token = json
            .get("port")
            .ok_or_else(|| CoreError::other("Missing or invalid 'port' field"))?;
        let port = parse_number_or_string_token(
            port_token,
            "port",
            "Invalid 'port' field type",
            |value| value as i32,
        )
        .map_err(|e| CoreError::other(e.to_string()))?;

        Ok(Self { address, port })
    }
}

fn parse_peer_list(json: &JObject, field: &str) -> CoreResult<Vec<RpcPeer>> {
    parse_optional_present_token_array_strict(json, field, |token| {
        token
            .as_object()
            .ok_or_else(|| CoreError::other(format!("{field} entry must be an object")))
            .and_then(RpcPeer::from_json)
    })
}

#[cfg(test)]
#[path = "../../../tests/client/models/network/rpc_peers.rs"]
mod tests;
