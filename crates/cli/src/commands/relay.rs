//! Relay command - relay a signed transaction

use super::CommandResult;
use neo_json::JToken;
use neo_rpc_client::RpcClient;

pub async fn execute(client: &RpcClient, transaction: &str) -> CommandResult {
    // The transaction can be hex or base64 encoded
    let tx_hex = if transaction
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == 'x')
    {
        transaction
            .strip_prefix("0x")
            .unwrap_or(transaction)
            .to_string()
    } else {
        // Assume base64
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, transaction)
                .map_err(|e| anyhow::anyhow!("Invalid transaction encoding: {}", e))?;
        hex::encode(decoded)
    };

    let params = vec![JToken::String(tx_hex)];
    let result = client
        .rpc_send_async("sendrawtransaction", params)
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output =
        serde_json::to_string_pretty(&result).map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
