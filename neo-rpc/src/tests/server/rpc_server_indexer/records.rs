use super::super::RpcServerIndexer;
use super::support::{account, block, execution, find_handler, transaction, transfer_state};
use crate::server::rpc_server::RpcServer;
use crate::server::rpc_server_settings::RpcServerConfig;
use neo_config::ProtocolSettings;
use neo_indexer::IndexerService;
use neo_primitives::UInt256;
use serde_json::Value;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn indexer_methods_return_indexed_records() {
    let service = Arc::new(IndexerService::new());
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_indexer(Arc::clone(&service)),
    );

    let first = account(8);
    let second = account(2);
    let recipient = account(3);
    let tx0 = transaction(10, &[first]);
    let tx1 = transaction(11, &[first, second]);
    let tx1_hash = tx1.try_hash().expect("tx hash");
    let contract = account(9);
    let block = block(7, vec![tx0, tx1.clone()]);
    let block_hash = block.try_hash().expect("block hash");
    service
        .index_block_with_application_executions(
            &block,
            &[execution(
                tx1,
                contract,
                "Transfer",
                transfer_state(first, recipient, 7),
            )],
        )
        .expect("index block");

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();

    let status =
        (find_handler(&handlers, "getindexerstatus").callback())(&server, &[]).expect("status");
    assert_eq!(status["indexedheight"].as_u64(), Some(7));
    assert_eq!(
        status["indexedhash"].as_str(),
        Some(block_hash.to_string().as_str())
    );
    assert_eq!(status["indexedblocks"].as_u64(), Some(1));
    assert_eq!(status["indexedtransactions"].as_u64(), Some(2));
    assert_eq!(status["indexedaccounts"].as_u64(), Some(2));
    assert_eq!(status["indexednotifications"].as_u64(), Some(1));
    assert_eq!(status["indexednotificationaccounts"].as_u64(), Some(2));
    assert_eq!(status["persistent"].as_bool(), Some(false));
    assert_eq!(status["persistencemode"].as_str(), Some("memory"));
    assert!(status["storepath"].is_null());
    assert_eq!(status["ledgerheight"].as_u64(), Some(0));
    assert_eq!(status["blocksbehind"].as_u64(), Some(0));
    assert_eq!(status["synced"].as_bool(), Some(false));
    assert_eq!(status["applicationlogs"]["enabled"].as_bool(), Some(false));
    assert_eq!(
        status["applicationlogs"]["notificationrecovery"].as_bool(),
        Some(false)
    );

    let block_by_height =
        (find_handler(&handlers, "getblockindex").callback())(&server, &[Value::from(7_u64)])
            .expect("block index by height");
    assert_eq!(
        block_by_height["hash"].as_str(),
        Some(block_hash.to_string().as_str())
    );
    assert_eq!(block_by_height["height"].as_u64(), Some(7));
    assert_eq!(block_by_height["txcount"].as_u64(), Some(2));

    let block_indexes = (find_handler(&handlers, "getblockindexes").callback())(
        &server,
        &[Value::from(0_u64), Value::from(1_u64)],
    )
    .expect("block indexes");
    let block_indexes = block_indexes.as_array().expect("block index array");
    assert_eq!(block_indexes.len(), 1);
    assert_eq!(
        block_indexes[0]["hash"].as_str(),
        Some(block_hash.to_string().as_str())
    );
    assert_eq!(block_indexes[0]["height"].as_u64(), Some(7));

    let block_by_hash = (find_handler(&handlers, "getblockindex").callback())(
        &server,
        &[Value::String(block_hash.to_string())],
    )
    .expect("block index by hash");
    assert_eq!(block_by_hash["height"].as_u64(), Some(7));

    let tx_index = (find_handler(&handlers, "gettransactionindex").callback())(
        &server,
        &[Value::String(tx1_hash.to_string())],
    )
    .expect("transaction index");
    assert_eq!(
        tx_index["txid"].as_str(),
        Some(tx1_hash.to_string().as_str())
    );
    assert_eq!(tx_index["blockheight"].as_u64(), Some(7));
    assert_eq!(tx_index["txindex"].as_u64(), Some(1));
    assert_eq!(tx_index["signers"].as_array().expect("signers").len(), 2);

    let block_page = [Value::from(7_u64), Value::from(1_u64), Value::from(1_u64)];
    let block_transactions =
        (find_handler(&handlers, "getblocktransactions").callback())(&server, &block_page)
            .expect("block transactions");
    let block_transactions = block_transactions.as_array().expect("block tx array");
    assert_eq!(block_transactions.len(), 1);
    assert_eq!(
        block_transactions[0]["txid"].as_str(),
        Some(tx1_hash.to_string().as_str())
    );
    assert_eq!(block_transactions[0]["blockheight"].as_u64(), Some(7));
    assert_eq!(block_transactions[0]["txindex"].as_u64(), Some(1));

    let address = first.to_address_with_version(server.system().settings().address_version);
    let address_page = [
        Value::String(address),
        Value::from(0_u64),
        Value::from(1_u64),
    ];
    let first_page =
        (find_handler(&handlers, "getaddresstransactions").callback())(&server, &address_page)
            .expect("address transactions");
    let first_page = first_page.as_array().expect("address tx array");
    assert_eq!(first_page.len(), 1);
    assert_eq!(first_page[0]["blockheight"].as_u64(), Some(7));
    assert_eq!(first_page[0]["txindex"].as_u64(), Some(0));

    let recipient_address =
        recipient.to_address_with_version(server.system().settings().address_version);
    let recipient_transactions = (find_handler(&handlers, "getaddresstransactions").callback())(
        &server,
        &[Value::String(recipient_address.clone())],
    )
    .expect("recipient signer transactions");
    assert!(
        recipient_transactions
            .as_array()
            .expect("recipient tx array")
            .is_empty(),
        "recipient is a Transfer participant, not a signer"
    );

    let contract_notifications = (find_handler(&handlers, "getcontractnotifications").callback())(
        &server,
        &[
            Value::String(contract.to_string()),
            Value::String("Transfer".to_string()),
        ],
    )
    .expect("contract notifications");
    let contract_notifications = contract_notifications
        .as_array()
        .expect("contract notification array");
    assert_eq!(contract_notifications.len(), 1);
    assert_eq!(
        contract_notifications[0]["txid"].as_str(),
        Some(tx1_hash.to_string().as_str())
    );
    assert_eq!(
        contract_notifications[0]["eventname"].as_str(),
        Some("Transfer")
    );
    assert_eq!(
        contract_notifications[0]["stateitemcount"].as_u64(),
        Some(3)
    );
    assert_eq!(
        contract_notifications[0]["state"][0]["type"].as_str(),
        Some("ByteString")
    );
    assert_eq!(
        contract_notifications[0]["state"][2]["value"].as_str(),
        Some("7")
    );
    let notification_accounts = contract_notifications[0]["accounts"]
        .as_array()
        .expect("accounts");
    assert_eq!(notification_accounts.len(), 2);
    assert_eq!(
        notification_accounts[0]["account"].as_str(),
        Some(first.to_string().as_str())
    );
    assert_eq!(
        notification_accounts[1]["account"].as_str(),
        Some(recipient.to_string().as_str())
    );

    let contract_transactions = (find_handler(&handlers, "getcontracttransactions").callback())(
        &server,
        &[
            Value::String(contract.to_string()),
            Value::String("Transfer".to_string()),
        ],
    )
    .expect("contract transactions");
    let contract_transactions = contract_transactions
        .as_array()
        .expect("contract transaction array");
    assert_eq!(contract_transactions.len(), 1);
    assert_eq!(
        contract_transactions[0]["txid"].as_str(),
        Some(tx1_hash.to_string().as_str())
    );
    assert_eq!(contract_transactions[0]["blockheight"].as_u64(), Some(7));
    assert_eq!(contract_transactions[0]["txindex"].as_u64(), Some(1));

    let recipient_notifications = (find_handler(&handlers, "getaddressnotifications").callback())(
        &server,
        &[Value::String(recipient_address)],
    )
    .expect("recipient notifications");
    let recipient_notifications = recipient_notifications
        .as_array()
        .expect("recipient notification array");
    assert_eq!(recipient_notifications.len(), 1);
    assert_eq!(
        recipient_notifications[0]["txid"].as_str(),
        Some(tx1_hash.to_string().as_str())
    );

    let tx_notifications = (find_handler(&handlers, "gettransactionnotifications").callback())(
        &server,
        &[Value::String(tx1_hash.to_string())],
    )
    .expect("transaction notifications");
    assert_eq!(
        tx_notifications
            .as_array()
            .expect("tx notification array")
            .len(),
        1
    );

    let block_notifications = (find_handler(&handlers, "getblocknotifications").callback())(
        &server,
        &[Value::from(7_u64)],
    )
    .expect("block notifications");
    assert_eq!(
        block_notifications
            .as_array()
            .expect("block notification array")
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn indexer_methods_return_null_for_missing_records() {
    let service = Arc::new(IndexerService::new());
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_indexer(service),
    );

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerIndexer::register_handlers();

    let missing_block =
        (find_handler(&handlers, "getblockindex").callback())(&server, &[Value::from(999_u64)])
            .expect("missing block");
    assert!(missing_block.is_null());

    let missing_tx = (find_handler(&handlers, "gettransactionindex").callback())(
        &server,
        &[Value::String(UInt256::zero().to_string())],
    )
    .expect("missing transaction");
    assert!(missing_tx.is_null());

    let missing_block_transactions = (find_handler(&handlers, "getblocktransactions").callback())(
        &server,
        &[Value::from(999_u64)],
    )
    .expect("missing block transactions");
    assert!(
        missing_block_transactions
            .as_array()
            .expect("missing block tx array")
            .is_empty()
    );

    let missing_contract_transactions = (find_handler(&handlers, "getcontracttransactions")
        .callback())(
        &server, &[Value::String(account(7).to_string())]
    )
    .expect("missing contract transactions");
    assert!(
        missing_contract_transactions
            .as_array()
            .expect("missing contract tx array")
            .is_empty()
    );
}
