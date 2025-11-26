//! Balance command - retrieves NEP-17 token balances

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, address: &str) -> CommandResult {
    let balances = client.get_nep17_balances(address).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output = serde_json::to_string_pretty(&balances)
        .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
