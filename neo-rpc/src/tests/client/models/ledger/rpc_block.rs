use super::*;
use crate::types::test_fixtures::rpc_case_result;

#[test]
fn block_to_json_matches_rpc_test_case() {
    let Some(expected) = rpc_case_result("getblockasync") else {
        return;
    };
    let settings = ProtocolSettings::default_settings();
    let parsed = RpcBlock::from_json(&expected, &settings).expect("parse");
    let actual = parsed.to_json(&settings);
    assert_eq!(expected.to_string(), actual.to_string());
}
