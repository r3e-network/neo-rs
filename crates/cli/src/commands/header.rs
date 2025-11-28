//! Header command - retrieves block header information

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, index_or_hash: &str, raw: bool) -> CommandResult {
    if raw {
        let hex = client
            .get_block_header_hex(index_or_hash)
            .await
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;
        Ok(hex)
    } else {
        let header = client
            .get_block_header(index_or_hash)
            .await
            .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

        let output = serde_json::to_string_pretty(&header)
            .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;
        Ok(output)
    }
}
