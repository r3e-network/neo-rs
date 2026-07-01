use super::*;
use std::collections::HashSet;

#[test]
fn rpc_error_access_denied_json() {
    let json = RpcError::access_denied().to_json().to_string();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    assert_eq!(parsed.get("code").and_then(|v| v.as_f64()), Some(-600.0));
    assert_eq!(
        parsed.get("message").and_then(|v| v.as_str()),
        Some("Access denied")
    );
}

#[test]
fn rpc_error_data_only_on_wallet_fee_limit() {
    let errors = vec![
        RpcError::invalid_request(),
        RpcError::method_not_found(),
        RpcError::invalid_params(),
        RpcError::internal_server_error(),
        RpcError::too_many_requests(),
        RpcError::bad_request(),
        RpcError::unknown_block(),
        RpcError::unknown_contract(),
        RpcError::unknown_transaction(),
        RpcError::unknown_storage_item(),
        RpcError::unknown_script_container(),
        RpcError::unknown_state_root(),
        RpcError::unknown_iterator(),
        RpcError::unknown_session(),
        RpcError::unknown_height(),
        RpcError::insufficient_funds_wallet(),
        RpcError::wallet_fee_limit(),
        RpcError::no_opened_wallet(),
        RpcError::wallet_not_found(),
        RpcError::wallet_not_supported(),
        RpcError::unknown_account(),
        RpcError::verification_failed(),
        RpcError::already_exists(),
        RpcError::mempool_cap_reached(),
        RpcError::already_in_pool(),
        RpcError::insufficient_network_fee(),
        RpcError::policy_failed(),
        RpcError::invalid_script(),
        RpcError::invalid_attribute(),
        RpcError::invalid_signature(),
        RpcError::invalid_size(),
        RpcError::expired_transaction(),
        RpcError::insufficient_funds(),
        RpcError::invalid_contract_verification(),
        RpcError::access_denied(),
        RpcError::sessions_disabled(),
        RpcError::oracle_disabled(),
        RpcError::oracle_request_finished(),
        RpcError::oracle_request_not_found(),
        RpcError::oracle_not_designated_node(),
        RpcError::unsupported_state(),
        RpcError::invalid_proof(),
        RpcError::execution_failed(),
    ];

    for error in errors.iter() {
        if error.code() == RpcError::wallet_fee_limit().code() {
            assert!(error.data().is_some());
        } else {
            assert!(error.data().is_none());
        }
    }
}

#[test]
fn rpc_error_wallet_fee_limit_json_includes_data() {
    let error = RpcError::wallet_fee_limit();
    let json = error.to_json().to_string();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse json");
    let data = parsed.get("data").and_then(|v| v.as_str()).expect("data");
    assert_eq!(data, error.data().expect("data"));
    let message = parsed
        .get("message")
        .and_then(|v| v.as_str())
        .expect("message");
    assert!(message.contains(error.message()));
    assert!(message.contains(data));
}

#[test]
fn rpc_error_strings_are_unique() {
    let errors = vec![
        RpcError::invalid_request(),
        RpcError::method_not_found(),
        RpcError::invalid_params(),
        RpcError::internal_server_error(),
        RpcError::too_many_requests(),
        RpcError::bad_request(),
        RpcError::unknown_block(),
        RpcError::unknown_contract(),
        RpcError::unknown_transaction(),
        RpcError::unknown_storage_item(),
        RpcError::unknown_script_container(),
        RpcError::unknown_state_root(),
        RpcError::unknown_iterator(),
        RpcError::unknown_session(),
        RpcError::unknown_height(),
        RpcError::insufficient_funds_wallet(),
        RpcError::wallet_fee_limit(),
        RpcError::no_opened_wallet(),
        RpcError::wallet_not_found(),
        RpcError::wallet_not_supported(),
        RpcError::unknown_account(),
        RpcError::verification_failed(),
        RpcError::already_exists(),
        RpcError::mempool_cap_reached(),
        RpcError::already_in_pool(),
        RpcError::insufficient_network_fee(),
        RpcError::policy_failed(),
        RpcError::invalid_script(),
        RpcError::invalid_attribute(),
        RpcError::invalid_signature(),
        RpcError::invalid_size(),
        RpcError::expired_transaction(),
        RpcError::insufficient_funds(),
        RpcError::invalid_contract_verification(),
        RpcError::access_denied(),
        RpcError::sessions_disabled(),
        RpcError::oracle_disabled(),
        RpcError::oracle_request_finished(),
        RpcError::oracle_request_not_found(),
        RpcError::oracle_not_designated_node(),
        RpcError::unsupported_state(),
        RpcError::invalid_proof(),
        RpcError::execution_failed(),
    ];

    let mut seen = HashSet::new();
    for error in errors {
        let key = error.to_string();
        assert!(seen.insert(key));
    }
}
