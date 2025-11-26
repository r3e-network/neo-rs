//! Test invoke command - test invokes a script

use super::CommandResult;
use neo_rpc_client::RpcClient;
use base64::{Engine, engine::general_purpose::STANDARD};

pub async fn execute(client: &RpcClient, script_b64: &str) -> CommandResult {
    let script = STANDARD.decode(script_b64)
        .map_err(|e| anyhow::anyhow!("Invalid base64 script: {}", e))?;

    let result = client.invoke_script(&script).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    Ok(format!("{:#?}", result))
}
