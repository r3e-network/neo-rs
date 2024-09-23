use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use crate::config;
use crate::crypto::keys::PublicKey;
use crate::encoding::fixedn::Fixed8;
use crate::result::version::{Version, Protocol, RPC};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Version {
    tcpport: u16,
    wsport: u16,
    nonce: u32,
    useragent: String,
    rpc: RPC,
    protocol: Protocol,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct RPC {
    maxiteratorresultitems: u32,
    sessionenabled: bool,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Protocol {
    addressversion: u8,
    network: u32,
    msperblock: u32,
    maxtraceableblocks: u32,
    maxvaliduntilblockincrement: u32,
    maxtransactionsperblock: u32,
    memorypoolmaxtransactions: u32,
    validatorscount: u32,
    initialgasdistribution: Fixed8,
    hardforks: HashMap<String, u32>,
    seedlist: Vec<String>,
    standbycommittee: Vec<PublicKey>,
}

#[test]
fn test_version_marshal_unmarshal_json() {
    let response_from_go_old = json!({
        "network": 860833102,
        "nonce": 1677922561,
        "protocol": {
            "addressversion": 53,
            "initialgasdistribution": "52000000",
            "maxtraceableblocks": 2102400,
            "maxtransactionsperblock": 512,
            "maxvaliduntilblockincrement": 5760,
            "memorypoolmaxtransactions": 50000,
            "msperblock": 15000,
            "network": 860833102,
            "validatorscount": 7
        },
        "tcpport": 10333,
        "useragent": "/NEO-GO:0.98.2/",
        "wsport": 10334
    });

    let response_from_go_new = json!({
        "nonce": 1677922561,
        "protocol": {
            "addressversion": 53,
            "initialgasdistribution": 5200000000000000u64,
            "maxtraceableblocks": 2102400,
            "maxtransactionsperblock": 512,
            "maxvaliduntilblockincrement": 5760,
            "memorypoolmaxtransactions": 50000,
            "msperblock": 15000,
            "network": 860833102,
            "validatorscount": 7,
            "hardforks": [{"name": "Aspidochelone", "blockheight": 123}, {"name": "Basilisk", "blockheight": 1234}],
            "seedlist": ["seed1.neo.org:10333", "seed2.neo.org:10333"],
            "standbycommittee": ["03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c", "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093", "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a"]
        },
        "rpc": {
            "maxiteratorresultitems": 100,
            "sessionenabled": true
        },
        "tcpport": 10333,
        "useragent": "/NEO-GO:0.98.6/",
        "wsport": 10334
    });

    let response_from_sharp = json!({
        "nonce": 1677922561,
        "protocol": {
            "addressversion": 53,
            "initialgasdistribution": 5200000000000000u64,
            "maxtraceableblocks": 2102400,
            "maxtransactionsperblock": 512,
            "maxvaliduntilblockincrement": 5760,
            "memorypoolmaxtransactions": 50000,
            "msperblock": 15000,
            "network": 860833102,
            "validatorscount": 7,
            "hardforks": [{"name": "HF_Aspidochelone", "blockheight": 123}, {"name": "HF_Basilisk", "blockheight": 1234}],
            "seedlist": ["seed1.neo.org:10333", "seed2.neo.org:10333"],
            "standbycommittee": ["03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c", "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093", "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a"]
        },
        "rpc": {
            "maxiteratorresultitems": 100,
            "sessionenabled": true
        },
        "tcpport": 10333,
        "useragent": "/Neo:3.1.0/",
        "wsport": 10334
    });

    let standby_committee = vec![
        PublicKey::from_hex("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap(),
        PublicKey::from_hex("02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093").unwrap(),
        PublicKey::from_hex("03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a").unwrap(),
    ];

    let v = Version {
        tcpport: 10333,
        wsport: 10334,
        nonce: 1677922561,
        useragent: "/NEO-GO:0.98.6/".to_string(),
        rpc: RPC {
            maxiteratorresultitems: 100,
            sessionenabled: true,
        },
        protocol: Protocol {
            addressversion: 53,
            network: 860833102,
            msperblock: 15000,
            maxtraceableblocks: 2102400,
            maxvaliduntilblockincrement: 5760,
            maxtransactionsperblock: 512,
            memorypoolmaxtransactions: 50000,
            validatorscount: 7,
            initialgasdistribution: Fixed8::from(52000000),
            hardforks: {
                let mut map = HashMap::new();
                map.insert("Aspidochelone".to_string(), 123);
                map.insert("Basilisk".to_string(), 1234);
                map
            },
            seedlist: vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
            ],
            standbycommittee: standby_committee,
        },
    };

    // MarshalJSON
    let actual = serde_json::to_string(&v).unwrap();
    assert_eq!(response_from_go_new.to_string(), actual);

    // UnmarshalJSON
    // Go node response
    // old RPC server
    let actual: Result<Version, _> = serde_json::from_str(&response_from_go_old.to_string());
    assert!(actual.is_err());

    // new RPC server
    let actual: Version = serde_json::from_str(&response_from_go_new.to_string()).unwrap();
    assert_eq!(v, actual);

    // Sharp node response
    let actual: Version = serde_json::from_str(&response_from_sharp.to_string()).unwrap();
    let mut expected = v.clone();
    expected.useragent = "/Neo:3.1.0/".to_string();
    assert_eq!(expected, actual);
}
