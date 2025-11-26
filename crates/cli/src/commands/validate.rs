//! ValidateAddress command - validates a Neo address

use super::CommandResult;
use neo_json::JToken;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, address: &str) -> CommandResult {
    let params = vec![JToken::String(address.to_string())];
    let result = client.rpc_send_async("validateaddress", params).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
