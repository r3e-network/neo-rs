//! Tools - parse, sign, and utility commands

use super::CommandResult;
use neo_rpc_client::RpcClient;

pub async fn parse(value: &str) -> CommandResult {
    let mut output = String::new();

    // Try to detect the type and parse accordingly
    output.push_str(&format!("Input: {}\n\n", value));

    // Check if it's a Neo address (starts with N and is 34 chars)
    if value.starts_with('N') && value.len() == 34 {
        output.push_str("Type: Neo Address\n");
        output.push_str(&format!("Address: {}\n", value));
        // TODO: Convert to script hash
    }
    // Check if it's a hex string (script hash or public key)
    else if value.starts_with("0x") || value.chars().all(|c| c.is_ascii_hexdigit()) {
        let hex_value = value.strip_prefix("0x").unwrap_or(value);
        output.push_str(&format!("Hex value: {}\n", hex_value));
        output.push_str(&format!("Length: {} bytes\n", hex_value.len() / 2));

        match hex_value.len() {
            40 => output.push_str("Type: Script Hash (UInt160)\n"),
            64 => output.push_str("Type: Hash256 (UInt256)\n"),
            66 => output.push_str("Type: Public Key (compressed)\n"),
            130 => output.push_str("Type: Public Key (uncompressed)\n"),
            _ => output.push_str("Type: Unknown hex data\n"),
        }
    }
    // Check if it's a WIF private key
    else if (value.starts_with('K') || value.starts_with('L') || value.starts_with('5')) && value.len() == 52 {
        output.push_str("Type: WIF Private Key\n");
        output.push_str("Warning: Do not share private keys!\n");
    }
    // Check if it's base64
    else if value.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
        if let Ok(decoded) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, value) {
            output.push_str("Type: Base64 encoded\n");
            output.push_str(&format!("Decoded length: {} bytes\n", decoded.len()));
            output.push_str(&format!("Hex: {}\n", hex::encode(&decoded)));
        }
    }
    else {
        output.push_str("Type: Unknown\n");
    }

    Ok(output.trim_end().to_string())
}

pub async fn parse_script(client: &RpcClient, script_b64: &str) -> CommandResult {
    // Decode the base64 script
    let script = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, script_b64)
        .map_err(|e| anyhow::anyhow!("Invalid base64: {}", e))?;

    let mut output = String::new();
    output.push_str(&format!("Script length: {} bytes\n", script.len()));
    output.push_str(&format!("Hex: {}\n\n", hex::encode(&script)));

    // Try to invoke the script to get more info
    match client.invoke_script(&script).await {
        Ok(result) => {
            output.push_str("Invoke result:\n");
            output.push_str(&format!("{:#?}\n", result));
        }
        Err(e) => {
            output.push_str(&format!("Could not invoke script: {}\n", e));
        }
    }

    Ok(output.trim_end().to_string())
}

pub async fn sign(data: &str, _key: &str) -> CommandResult {
    // Signing requires cryptographic operations
    // For security, this should be done with proper key management
    Ok(format!(
        "Sign command requires local key management.\n\
        Data to sign: {} bytes\n\n\
        Note: For security, signing operations should be performed locally.",
        data.len() / 2
    ))
}
