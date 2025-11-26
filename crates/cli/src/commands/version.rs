//! Version command - displays node version information

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient) -> CommandResult {
    let result = client.rpc_send_async("getversion", vec![]).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
