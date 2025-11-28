//! Transfers command - retrieves NEP-17 transfer history

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(
    client: &RpcClient,
    address: &str,
    from: Option<u64>,
    to: Option<u64>,
) -> CommandResult {
    let transfers = client
        .get_nep17_transfers(address, from, to)
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output = serde_json::to_string_pretty(&transfers)
        .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
