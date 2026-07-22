use super::*;
use crate::types::test_fixtures::rpc_case_result;

fn sample_protocol() -> RpcProtocol {
    let mut hardforks = BTreeMap::new();
    hardforks.insert("neo3".to_string(), 0);

    RpcProtocol {
        network: 5195086,
        validators_count: 7,
        milliseconds_per_block: 15_000,
        max_valid_until_block_increment: 10,
        max_traceable_blocks: 100_000,
        address_version: 53,
        max_transactions_per_block: 512,
        memory_pool_max_transactions: 50_000,
        initial_gas_distribution: 5_200_000_000_000_000,
        hardforks,
        seed_list: vec!["seed1".into(), "seed2".into()],
        standby_committee: vec!["comm1".into(), "comm2".into()],
    }
}

#[test]
fn rpc_version_roundtrip() {
    let version = RpcVersion {
        tcp_port: 10333,
        nonce: 42,
        user_agent: "/NEO:3.6/".into(),
        protocol: sample_protocol(),
    };

    let json = version.to_json();
    let parsed = RpcVersion::from_json(&json).expect("version");

    assert_eq!(parsed.tcp_port, version.tcp_port);
    assert_eq!(parsed.nonce, version.nonce);
    assert_eq!(parsed.user_agent, version.user_agent);
    assert_eq!(parsed.protocol.network, version.protocol.network);
    assert_eq!(
        parsed.protocol.hardforks.get("neo3"),
        version.protocol.hardforks.get("neo3")
    );
    assert_eq!(parsed.protocol.seed_list.len(), 2);
    assert_eq!(parsed.protocol.standby_committee.len(), 2);
}

#[test]
fn version_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getversionasync") else {
        return;
    };
    let parsed = RpcVersion::from_json(&expected).expect("parse");
    let actual = parsed.to_json();
    assert_eq!(expected.to_string(), actual.to_string());
}
