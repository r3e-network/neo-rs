//! State command - displays node state information

use std::sync::Arc;

use super::CommandResult;
use neo_rpc_client::{RpcClient, StateApi};

pub async fn execute(client: &RpcClient) -> CommandResult {
    let block_count = client
        .get_block_count()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get block count: {}", e))?;

    let best_hash = client
        .get_best_block_hash()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get best block hash: {}", e))?;

    let header_count = client
        .get_block_header_count()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get header count: {}", e))?;

    let state_api = StateApi::new(Arc::new(client.clone()));
    let (local, validated) = state_api
        .get_state_height()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get state height: {}", e))?;

    let state_root = if let Some(height) = validated.or(local) {
        match state_api.get_state_root(height).await {
            Ok(root) => format!(
                "State Root [{}]: {} (witness: {})",
                root.index,
                root.root_hash,
                root.witness.as_ref().map(|_| "yes").unwrap_or("no")
            ),
            Err(err) => format!("State Root: <error: {err}>"),
        }
    } else {
        "State Root: <unavailable>".to_string()
    };

    Ok(format!(
        "\
Block Height: {block_height}
Header Height: {header_height}
Best Block Hash: {best_hash}
Local State Root Index: {local_index}
Validated State Root Index: {validated_index}
{state_root}",
        block_height = block_count.saturating_sub(1),
        header_height = header_count.saturating_sub(1),
        best_hash = best_hash,
        local_index = local
            .map(|v| v.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        validated_index = validated
            .map(|v| v.to_string())
            .unwrap_or_else(|| "<unknown>".to_string()),
        state_root = state_root
    ))
}
