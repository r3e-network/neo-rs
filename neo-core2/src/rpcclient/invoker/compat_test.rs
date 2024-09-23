use std::sync::Arc;
use std::sync::Mutex;
use crate::rpcclient::{Client, WSClient};
use crate::rpcclient::invoker::{RPCInvoke, RPCInvokeHistoric, RPCSessions};

#[test]
fn test_rpc_invoker_rpc_client_compat() {
    let client = Arc::new(Mutex::new(Client::new()));
    let ws_client = Arc::new(Mutex::new(WSClient::new()));

    let _ = RPCInvoke(client.clone());
    let _ = RPCInvoke(ws_client.clone());
    let _ = RPCInvokeHistoric(client.clone());
    let _ = RPCInvokeHistoric(ws_client.clone());
    let _ = RPCSessions(ws_client.clone());
}
