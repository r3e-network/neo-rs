//! Native contract commands

use super::CommandResult;
use crate::{GasTokenCmd, NeoTokenCmd};
use neo_json::JToken;
use neo_rpc_client::RpcClient;

/// NEO token contract hash (mainnet)
const NEO_TOKEN: &str = "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5";
/// GAS token contract hash (mainnet)
const GAS_TOKEN: &str = "0xd2a4cff31913016155e38e474a2c06d08be276cf";

pub async fn list_contracts(client: &RpcClient) -> CommandResult {
    let result = client
        .rpc_send_async("getnativecontracts", vec![])
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output =
        serde_json::to_string_pretty(&result).map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}

pub async fn neo_token(client: &RpcClient, cmd: NeoTokenCmd) -> CommandResult {
    token_command(client, NEO_TOKEN, cmd.into()).await
}

pub async fn gas_token(client: &RpcClient, cmd: GasTokenCmd) -> CommandResult {
    token_command(client, GAS_TOKEN, cmd.into()).await
}

enum TokenCommand {
    TotalSupply,
    Decimals,
    Symbol,
    BalanceOf(String),
}

impl From<NeoTokenCmd> for TokenCommand {
    fn from(cmd: NeoTokenCmd) -> Self {
        match cmd {
            NeoTokenCmd::TotalSupply => TokenCommand::TotalSupply,
            NeoTokenCmd::Decimals => TokenCommand::Decimals,
            NeoTokenCmd::Symbol => TokenCommand::Symbol,
            NeoTokenCmd::BalanceOf { address } => TokenCommand::BalanceOf(address),
        }
    }
}

impl From<GasTokenCmd> for TokenCommand {
    fn from(cmd: GasTokenCmd) -> Self {
        match cmd {
            GasTokenCmd::TotalSupply => TokenCommand::TotalSupply,
            GasTokenCmd::Decimals => TokenCommand::Decimals,
            GasTokenCmd::Symbol => TokenCommand::Symbol,
            GasTokenCmd::BalanceOf { address } => TokenCommand::BalanceOf(address),
        }
    }
}

async fn token_command(client: &RpcClient, contract: &str, cmd: TokenCommand) -> CommandResult {
    let (method, params) = match cmd {
        TokenCommand::TotalSupply => ("totalSupply", vec![]),
        TokenCommand::Decimals => ("decimals", vec![]),
        TokenCommand::Symbol => ("symbol", vec![]),
        TokenCommand::BalanceOf(address) => {
            let param: neo_json::JObject = vec![
                (
                    "type".to_string(),
                    Some(JToken::String("Hash160".to_string())),
                ),
                ("value".to_string(), Some(JToken::String(address))),
            ]
            .into_iter()
            .collect();
            ("balanceOf", vec![JToken::Object(param)])
        }
    };

    let invoke_params = vec![
        JToken::String(contract.to_string()),
        JToken::String(method.to_string()),
        JToken::Array(params.into()),
    ];

    let result = client
        .rpc_send_async("invokefunction", invoke_params)
        .await
        .map_err(|e| anyhow::anyhow!("RPC error: {}", e))?;

    let output =
        serde_json::to_string_pretty(&result).map_err(|e| anyhow::anyhow!("JSON error: {}", e))?;

    Ok(output)
}
