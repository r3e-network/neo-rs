//! Block command - retrieves block information

use super::CommandResult;
use neo_rpc::client::RpcClient;

pub async fn execute(client: &RpcClient, index_or_hash: &str, raw: bool) -> CommandResult {
    if raw {
        let hex = client
            .get_block_hex(index_or_hash)
            .await
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;
        Ok(hex)
    } else {
        let block = client
            .get_block(index_or_hash)
            .await
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

        let output = serde_json::to_string_pretty(&block)
            .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;
        Ok(output)
    }
}
