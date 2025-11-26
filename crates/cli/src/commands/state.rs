//! State command - displays node state information

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient) -> CommandResult {
    let block_count = client.get_block_count().await
        .map_err(|e| anyhow::anyhow!("Failed to get block count: {}", e))?;

    let best_hash = client.get_best_block_hash().await
        .map_err(|e| anyhow::anyhow!("Failed to get best block hash: {}", e))?;

    let header_count = client.get_block_header_count().await
        .map_err(|e| anyhow::anyhow!("Failed to get header count: {}", e))?;

    Ok(format!(
        "Block Height: {}\nHeader Height: {}\nBest Block Hash: {}",
        block_count.saturating_sub(1),
        header_count.saturating_sub(1),
        best_hash
    ))
}
