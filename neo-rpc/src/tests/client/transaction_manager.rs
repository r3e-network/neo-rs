use super::*;
use mockito::{Matcher, Server};
use neo_config::ProtocolSettings;
use neo_primitives::WitnessScope;
use neo_serialization::json::{JArray, JObject, JToken};
use reqwest::Url;
use std::net::TcpListener;

fn localhost_binding_permitted() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
}

fn rpc_response(result: JToken) -> String {
    let mut response = JObject::new();
    response.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
    response.insert("id".to_string(), JToken::Number(1.0));
    response.insert("result".to_string(), result);
    JToken::Object(response).to_string()
}

fn invoke_result_payload(gas_consumed: i64, balance: &str) -> JObject {
    let mut result = JObject::new();
    result.insert("script".to_string(), JToken::String("AA==".to_string()));
    result.insert("state".to_string(), JToken::String("HALT".to_string()));
    result.insert(
        "gasconsumed".to_string(),
        JToken::String(gas_consumed.to_string()),
    );

    let mut stack_item = JObject::new();
    stack_item.insert("type".to_string(), JToken::String("Integer".to_string()));
    stack_item.insert("value".to_string(), JToken::String(balance.to_string()));
    let stack = JArray::from(vec![JToken::Object(stack_item)]);
    result.insert("stack".to_string(), JToken::Array(stack));
    result
}

fn mock_invokescript(server: &mut Server, response_body: &str) {
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .expect_at_least(1)
        .create();
}

fn mock_block_count(server: &mut Server, count: u32) {
    let response = rpc_response(JToken::Number(count as f64));
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"getblockcount""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response)
        .expect(1)
        .create();
}

fn mock_calculate_network_fee(server: &mut Server, fee: i64) {
    let mut result = JObject::new();
    result.insert("networkfee".to_string(), JToken::Number(fee as f64));
    let response = rpc_response(JToken::Object(result));
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"calculatenetworkfee""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response)
        .expect(1)
        .create();
}

fn mock_calculate_network_fee_with_hits(server: &mut Server, fee: i64, hits: usize) {
    let mut result = JObject::new();
    result.insert("networkfee".to_string(), JToken::Number(fee as f64));
    let response = rpc_response(JToken::Object(result));
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"calculatenetworkfee""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response)
        .expect(hits)
        .create();
}

#[tokio::test]
async fn make_transaction_preserves_signer_scope() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    let invoke_result = invoke_result_payload(100, "10000000000000000");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(RpcClient::builder(url).build().unwrap());
    let key = KeyPair::from_private_key(&[1u8; 32]).expect("key");
    let sender = key.script_hash();
    let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

    let manager = TransactionManager::make_transaction(client, &[0x01], Some(signers), None)
        .await
        .expect("manager");

    assert_eq!(manager.tx().signers()[0].scopes(), WitnessScope::GLOBAL);
}

#[tokio::test]
async fn sign_adds_signature_and_sets_fees() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    mock_calculate_network_fee(&mut server, 100_000_000);
    let invoke_result = invoke_result_payload(100, "10000000000000000");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(
        RpcClient::builder(url)
            .protocol_settings(ProtocolSettings::default_settings())
            .build()
            .unwrap(),
    );
    let key = KeyPair::from_private_key(&[2u8; 32]).expect("key");
    let sender = key.script_hash();
    let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

    let mut manager =
        TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
            .await
            .expect("manager");
    manager.add_signature(&key).expect("add signature");
    let tx = manager.sign().await.expect("sign");

    assert_eq!(tx.network_fee(), 100_000_000);
    assert_eq!(tx.system_fee(), 100);
    assert_eq!(tx.witnesses().len(), 1);

    let invocation = tx.witnesses()[0].invocation_script();
    assert_eq!(invocation.len(), 66);
    let signature = &invocation[2..];
    let sign_data = get_sign_data_vec(&tx, client.protocol_settings.network).expect("sign data");
    assert!(key.verify(&sign_data, signature).expect("verify signature"));
}

#[tokio::test]
async fn sign_rejects_mismatched_key() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    mock_calculate_network_fee(&mut server, 100_000_000);
    let invoke_result = invoke_result_payload(100, "10000000000000000");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(RpcClient::builder(url).build().unwrap());
    let key = KeyPair::from_private_key(&[3u8; 32]).expect("key");
    let wrong_key = KeyPair::from_private_key(&[4u8; 32]).expect("wrong key");
    let sender = key.script_hash();
    let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

    let mut manager =
        TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
            .await
            .expect("manager");
    let err = manager
        .add_signature(&wrong_key)
        .err()
        .expect("mismatched key");
    assert!(err.to_string().contains("Mismatch ScriptHash"));
}

#[tokio::test]
async fn sign_rejects_duplicate_signature() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    mock_calculate_network_fee_with_hits(&mut server, 100_000_000, 2);
    let invoke_result = invoke_result_payload(100, "10000000000000000");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(
        RpcClient::builder(url)
            .protocol_settings(ProtocolSettings::default_settings())
            .build()
            .unwrap(),
    );
    let key = KeyPair::from_private_key(&[9u8; 32]).expect("key");
    let sender = key.script_hash();
    let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

    let mut manager =
        TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
            .await
            .expect("manager");
    manager.add_signature(&key).expect("add signature");
    manager.sign().await.expect("sign");

    manager.add_signature(&key).expect("add signature again");
    let err = manager.sign().await.expect_err("duplicate signature");
    assert!(err.to_string().contains("AddSignature failed"));
    assert!(manager.tx().witnesses().is_empty());
}

#[tokio::test]
async fn sign_rejects_insufficient_gas() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    mock_calculate_network_fee(&mut server, 100_000_000);
    let invoke_result = invoke_result_payload(100, "1");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(
        RpcClient::builder(url)
            .protocol_settings(ProtocolSettings::default_settings())
            .build()
            .unwrap(),
    );
    let key = KeyPair::from_private_key(&[8u8; 32]).expect("key");
    let sender = key.script_hash();
    let signers = vec![Signer::new(sender, WitnessScope::GLOBAL)];

    let mut manager =
        TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
            .await
            .expect("manager");
    manager.add_signature(&key).expect("add signature");

    let err = manager.sign().await.expect_err("insufficient gas");
    assert!(err.to_string().contains("Insufficient GAS"));
}

#[tokio::test]
async fn sign_multi_sig_contract() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    mock_calculate_network_fee(&mut server, 100_000_000);
    let invoke_result = invoke_result_payload(100, "10000000000000000");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(RpcClient::builder(url).build().unwrap());
    let key_a = KeyPair::from_private_key(&[5u8; 32]).expect("key a");
    let key_b = KeyPair::from_private_key(&[6u8; 32]).expect("key b");
    let pub_a = key_a.public_key_point().expect("pub a");
    let pub_b = key_b.public_key_point().expect("pub b");
    let contract = Contract::create_multi_sig_contract(2, &[pub_a.clone(), pub_b.clone()]);
    let signers = vec![Signer::new(contract.script_hash(), WitnessScope::GLOBAL)];

    let mut manager =
        TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
            .await
            .expect("manager");
    manager
        .add_multi_sig(&key_a, 2, vec![pub_a.clone(), pub_b.clone()])
        .expect("add multisig a");
    manager
        .add_multi_sig(&key_b, 2, vec![pub_a, pub_b])
        .expect("add multisig b");
    let tx = manager.sign().await.expect("sign");
    assert_eq!(tx.witnesses().len(), 1);
}

#[tokio::test]
async fn add_witness_by_hash_adds_second_witness() {
    if !localhost_binding_permitted() {
        return;
    }
    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 100);
    mock_calculate_network_fee(&mut server, 100_000_000);
    let invoke_result = invoke_result_payload(100, "10000000000000000");
    let response_body = rpc_response(JToken::Object(invoke_result));
    mock_invokescript(&mut server, &response_body);

    let url = Url::parse(&server.url()).unwrap();
    let client = Arc::new(RpcClient::builder(url).build().unwrap());
    let key = KeyPair::from_private_key(&[7u8; 32]).expect("key");
    let sender = key.script_hash();
    let signers = vec![
        Signer::new(sender, WitnessScope::GLOBAL),
        Signer::new(UInt160::zero(), WitnessScope::GLOBAL),
    ];

    let mut manager =
        TransactionManager::make_transaction(Arc::clone(&client), &[0x01], Some(signers), None)
            .await
            .expect("manager");
    manager
        .add_witness_with_hash(&UInt160::zero())
        .expect("add witness");
    manager.add_signature(&key).expect("add signature");
    let tx = manager.sign().await.expect("sign");
    assert_eq!(tx.witnesses().len(), 2);
    assert_eq!(tx.witnesses()[0].verification_script().len(), 40);
    assert_eq!(tx.witnesses()[0].invocation_script().len(), 66);
}
