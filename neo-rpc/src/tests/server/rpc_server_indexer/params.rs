use std::sync::Arc;

use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_indexer::IndexerService;
use neo_primitives::UInt256;
use serde_json::Value;

use super::super::RpcServerIndexer;
use super::super::support::STANDARD_PAGE_BOUNDS;
use super::support::{account, assert_invalid_params, find_handler};

#[test]
fn pagination_parser_defaults_caps_and_rejects_invalid_values() {
    let (skip, limit) =
        RpcServerIndexer::parse_page(&[], 0, STANDARD_PAGE_BOUNDS, "getblockindexes")
            .expect("default page");
    assert_eq!((skip, limit), (0, STANDARD_PAGE_BOUNDS.default_limit));

    let (skip, limit) = RpcServerIndexer::parse_page(
        &[Value::from(7_u64), Value::from(10_000_u64)],
        0,
        STANDARD_PAGE_BOUNDS,
        "getblockindexes",
    )
    .expect("capped page");
    assert_eq!((skip, limit), (7, STANDARD_PAGE_BOUNDS.max_limit));

    let err = RpcServerIndexer::parse_page(
        &[Value::from(-1_i64)],
        0,
        STANDARD_PAGE_BOUNDS,
        "getblockindexes",
    )
    .expect_err("negative skip should fail");
    assert!(
        err.to_string()
            .contains("getblockindexes expects unsigned integer"),
        "{err}"
    );

    let extra_page = [Value::from(0_u64), Value::from(10_u64), Value::from(20_u64)];
    let err = RpcServerIndexer::parse_page(&extra_page, 0, STANDARD_PAGE_BOUNDS, "getblockindexes")
        .expect_err("extra page parameter should fail");
    assert_invalid_params(err, "getblockindexes expects at most 2 parameters");
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_status_rejects_unexpected_parameters() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    system.register_service(Arc::new(IndexerService::new()));

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let err =
        (find_handler(&handlers, "getindexerstatus").callback())(&server, &[Value::from(1_u64)])
            .expect_err("getindexerstatus does not accept params");

    assert_eq!(
        err.code(),
        crate::server::rpc_error::RpcError::invalid_params().code()
    );
    assert!(
        err.to_string()
            .contains("getindexerstatus expects no parameters"),
        "{err}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_methods_reject_extra_parameters() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    system.register_service(Arc::new(IndexerService::new()));

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();
    let address_version = server.system().settings().address_version;
    let account = account(1);
    let address = account.to_address_with_version(address_version);
    let hash = UInt256::zero().to_string();
    let contract = account.to_string();

    let cases = vec![
        (
            "getblockindex",
            vec![Value::from(0_u64), Value::from(1_u64)],
            "getblockindex expects exactly 1 parameter",
        ),
        (
            "getblockindexes",
            vec![Value::from(0_u64), Value::from(100_u64), Value::from(1_u64)],
            "getblockindexes expects at most 2 parameters",
        ),
        (
            "gettransactionindex",
            vec![Value::String(hash.clone()), Value::from(1_u64)],
            "gettransactionindex expects exactly 1 parameter",
        ),
        (
            "getblocktransactions",
            vec![
                Value::from(0_u64),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getblocktransactions expects at most 3 parameters",
        ),
        (
            "getaddresstransactions",
            vec![
                Value::String(address.clone()),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getaddresstransactions expects at most 3 parameters",
        ),
        (
            "getcontracttransactions",
            vec![
                Value::String(contract.clone()),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getcontracttransactions expects at most 3 parameters",
        ),
        (
            "getcontracttransactions",
            vec![
                Value::String(contract.clone()),
                Value::String("Transfer".to_string()),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getcontracttransactions expects at most 4 parameters",
        ),
        (
            "getaddressnotifications",
            vec![
                Value::String(address),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getaddressnotifications expects at most 3 parameters",
        ),
        (
            "getblocknotifications",
            vec![
                Value::from(0_u64),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getblocknotifications expects at most 3 parameters",
        ),
        (
            "gettransactionnotifications",
            vec![
                Value::String(hash),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "gettransactionnotifications expects at most 3 parameters",
        ),
        (
            "getcontractnotifications",
            vec![
                Value::String(contract),
                Value::String("Transfer".to_string()),
                Value::from(0_u64),
                Value::from(100_u64),
                Value::from(1_u64),
            ],
            "getcontractnotifications expects at most 4 parameters",
        ),
    ];

    for (method, params, message) in cases {
        let err = (find_handler(&handlers, method).callback())(&server, &params)
            .expect_err("extra parameters should fail");
        assert_invalid_params(err, message);
    }
}
