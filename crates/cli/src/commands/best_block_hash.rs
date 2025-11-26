//! BestBlockHash command - gets the best block hash

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient) -> CommandResult {
    let hash = client.get_best_block_hash().await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    Ok(hash)
}
