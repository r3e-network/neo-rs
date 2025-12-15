//! BlockCount command - gets the block count (height + 1)

use super::CommandResult;
use neo_rpc::client::RpcClient;

pub async fn execute(client: &RpcClient) -> CommandResult {
    let count = client
        .get_block_count()
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    Ok(format!("{}", count))
}
