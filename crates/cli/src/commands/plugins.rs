//! Plugins command - lists loaded plugins

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient) -> CommandResult {
    let plugins = client
        .get_plugins()
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    if plugins.is_empty() {
        return Ok("No plugins loaded.".to_string());
    }

    let mut output = String::from("Loaded plugins:\n");
    for plugin in plugins {
        output.push_str(&format!("  - {} v{}\n", plugin.name, plugin.version));
        for interface in &plugin.interfaces {
            output.push_str(&format!("      interface: {}\n", interface));
        }
    }

    Ok(output.trim_end().to_string())
}
