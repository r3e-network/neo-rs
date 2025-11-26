//! Transaction command - retrieves transaction information

use super::CommandResult;
use neo_json::JToken;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, hash: &str, raw: bool) -> CommandResult {
    let verbose = if raw {
        JToken::Number(0.0)
    } else {
        JToken::Number(1.0)
    };

    let params = vec![JToken::String(hash.to_string()), verbose];
    let result = client.rpc_send_async("getrawtransaction", params).await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    if raw {
        // Raw mode returns hex string
        Ok(result.to_string().trim_matches('"').to_string())
    } else {
        let output = serde_json::to_string_pretty(&result)
            .map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;
        Ok(output)
    }
}
