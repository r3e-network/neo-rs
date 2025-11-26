//! Export blocks command

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(
    client: &RpcClient,
    path: &str,
    start: u32,
    count: u32,
) -> CommandResult {
    // Get current block height
    let block_count = client.get_block_count().await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let end = if count == 0 {
        block_count
    } else {
        std::cmp::min(start + count, block_count)
    };

    Ok(format!(
        "Export blocks command.\n\
        Path: {}\n\
        Start block: {}\n\
        End block: {}\n\
        Total blocks: {}\n\n\
        Note: Full block export requires local implementation.\n\
        For large exports, consider using neo-node directly.",
        path,
        start,
        end,
        end - start
    ))
}
