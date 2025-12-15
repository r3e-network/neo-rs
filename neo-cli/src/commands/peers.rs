//! Peers command - displays connected peer information

use super::CommandResult;
use neo_rpc::client::RpcClient;

pub async fn execute(client: &RpcClient) -> CommandResult {
    let result = client
        .rpc_send_async("getpeers", vec![])
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output =
        serde_json::to_string_pretty(&result).map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
