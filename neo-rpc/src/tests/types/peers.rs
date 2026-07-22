//! Tests for the transport-neutral peer-list RPC model.

use super::*;
use crate::types::test_fixtures::rpc_case_result;
use neo_serialization::json::{JArray, JToken};

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

    let mut incomplete_peer = JObject::new();
    incomplete_peer.insert(
        "address".to_string(),
        JToken::String("127.0.0.1".to_string()),
    );
    invalid.insert(
        "connected".to_string(),
        JToken::Array(JArray::from(vec![JToken::Object(incomplete_peer)])),
    );
    let parsed = RpcPeers::from_json(&invalid);
    assert_eq!(
        parsed.expect_err("peer parse error propagates").to_string(),
        "Missing or invalid 'port' field"
    );
}

#[test]
fn parse_peer_list_skips_empty_array_slots() {
    let mut peer = JObject::new();
    peer.insert(
        "address".to_string(),
        JToken::String("127.0.0.1".to_string()),
    );
    peer.insert("port".to_string(), JToken::Number(20333.0));

    let mut connected = JArray::new();
    connected.add(None);
    connected.add(Some(JToken::Object(peer)));

    let mut json = JObject::new();
    json.insert("connected".to_string(), JToken::Array(connected));

    let parsed = RpcPeers::from_json(&json).expect("peers");
    assert_eq!(parsed.connected.len(), 1);
    assert_eq!(parsed.connected[0].address, "127.0.0.1");
}

#[test]
fn peers_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getpeersasync") else {
        return;
    };
    let parsed = RpcPeers::from_json(&expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}
