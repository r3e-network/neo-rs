#![cfg(feature = "server")]

use neo_rpc::server::{RpcServerBlockchain, RpcServerWallet};

#[test]
fn blockchain_handler_registration_preserves_order_and_public_metadata() {
    let handlers = RpcServerBlockchain::register_handlers();
    let names: Vec<&str> = handlers
        .iter()
        .map(|handler| handler.descriptor().name.as_str())
        .collect();

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
    let names: Vec<&str> = handlers
        .iter()
        .map(|handler| handler.descriptor().name.as_str())
        .collect();

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
