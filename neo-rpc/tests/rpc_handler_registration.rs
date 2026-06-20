//! Regression tests for RPC handler registration metadata.
#![cfg(feature = "server")]

use neo_rpc::server::{RpcServerBlockchain, RpcServerIndexer, RpcServerWallet};

fn handler_names(handlers: &[neo_rpc::server::RpcHandler]) -> Vec<&str> {
    handlers
        .iter()
        .map(|handler| handler.descriptor().name.as_str())
        .collect()
}

#[test]
fn blockchain_handler_registration_preserves_order_and_public_metadata() {
    let handlers = RpcServerBlockchain::register_handlers();
    let names = handler_names(&handlers);

    assert_eq!(
        names,
        [
            "getbestblockhash",
            "getblockcount",
            "getblockheadercount",
            "getblockhash",
            "getblock",
            "getblockheader",
            "getblocksysfee",
            "getrawmempool",
            "getrawtransaction",
            "getcontractstate",
            "getstorage",
            "findstorage",
            "getnativecontracts",
            "getnextblockvalidators",
            "getcandidates",
            "gettransactionheight",
            "getcommittee",
        ]
    );
    assert!(
        handlers
            .iter()
            .all(|handler| !handler.descriptor().requires_auth())
    );
}

#[test]
fn wallet_handler_registration_preserves_protected_metadata() {
    let handlers = RpcServerWallet::register_handlers();
    let names = handler_names(&handlers);

    assert_eq!(
        names,
        [
            "closewallet",
            "dumpprivkey",
            "getnewaddress",
            "getwalletbalance",
            "getwalletunclaimedgas",
            "importprivkey",
            "listaddress",
            "openwallet",
            "calculatenetworkfee",
            "sendfrom",
            "sendtoaddress",
            "sendmany",
            "canceltransaction",
        ]
    );
    assert!(
        handlers
            .iter()
            .all(|handler| handler.descriptor().requires_auth())
    );
}

#[test]
fn indexer_handler_registration_preserves_order_and_public_metadata() {
    let handlers = RpcServerIndexer::register_handlers();
    let names = handler_names(&handlers);

    assert_eq!(
        names,
        [
            "getindexerstatus",
            "getblockindex",
            "getblockindexes",
            "gettransactionindex",
            "getblocktransactions",
            "getaddresstransactions",
            "getcontracttransactions",
            "getaddressnotifications",
            "getblocknotifications",
            "gettransactionnotifications",
            "getcontractnotifications",
        ]
    );
    assert!(
        handlers
            .iter()
            .all(|handler| !handler.descriptor().requires_auth())
    );
}

#[test]
fn indexer_rpc_docs_match_registered_handlers() {
    let docs = include_str!("../../docs/rpc-api.md");
    let indexer_section = docs
        .split("### NeoIndexer")
        .nth(1)
        .expect("rpc-api.md should document the NeoIndexer section")
        .split("\n### ")
        .next()
        .expect("NeoIndexer section should have content");
    let documented = indexer_section
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            if !line.starts_with("| `") {
                return None;
            }
            line.split('`').nth(1)
        })
        .collect::<Vec<_>>();

    let handlers = RpcServerIndexer::register_handlers();
    assert_eq!(documented, handler_names(&handlers));
}
