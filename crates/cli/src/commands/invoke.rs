//! Invoke command - invokes a contract method (read-only)

use super::CommandResult;
use neo_json::JToken;
use neo_rpc_client::RpcClient;

pub async fn execute(
    client: &RpcClient,
    hash: &str,
    method: &str,
    params_json: &str,
) -> CommandResult {
    // Parse params JSON array
    let params_token = JToken::parse(params_json, 64)
        .map_err(|e| anyhow::anyhow!("Invalid params JSON: {}", e))?;

    let params = match params_token {
        JToken::Array(arr) => arr.into_iter().collect::<Vec<_>>(),
        _ => return Err(anyhow::anyhow!("Params must be a JSON array")),
    };

    // Build invokefunction params: [scriptHash, operation, params]
    let invoke_params = vec![
        JToken::String(hash.to_string()),
        JToken::String(method.to_string()),
        JToken::Array(params.into()),
    ];

    let result = client.rpc_send_async("invokefunction", invoke_params).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output = serde_json::to_string_pretty(&result)
        .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
