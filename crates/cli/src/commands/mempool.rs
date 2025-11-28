//! Mempool command - displays memory pool status

use super::CommandResult;
use neo_json::JToken;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, verbose: bool) -> CommandResult {
    let params = if verbose {
        vec![JToken::Boolean(true)]
    } else {
        vec![]
    };

    let result = client
        .rpc_send_async("getrawmempool", params)
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output =
        serde_json::to_string_pretty(&result).map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
