//! BlockHash command - gets block hash by index

use super::CommandResult;
use neo_rpc::client::RpcClient;

pub async fn execute(client: &RpcClient, index: u32) -> CommandResult {
    let hash = client
        .get_block_hash(index)
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    Ok(hash)
}
