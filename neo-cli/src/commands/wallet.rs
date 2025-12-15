//! Wallet commands - manage wallets

use super::CommandResult;
use crate::WalletCommands;
use neo_rpc::client::RpcClient;

pub async fn execute(_client: &RpcClient, cmd: WalletCommands) -> CommandResult {
    // Note: Wallet operations require local wallet management
    // In the RPC-based architecture, we need either:
    // 1. A local wallet implementation in neo-cli
    // 2. RPC endpoints for wallet operations on neo-node
    //
    // For now, we provide a message indicating this limitation

    match cmd {
        WalletCommands::Open { path, password: _ } => Ok(format!(
            "Wallet open not yet implemented via RPC. Path: {}\n\
                Note: Consider using neo-node with wallet configuration.",
            path
        )),
        WalletCommands::Create { path } => Ok(format!(
            "Wallet create not yet implemented via RPC. Path: {}",
            path
        )),
        WalletCommands::List => Ok("Wallet list not yet implemented via RPC.".to_string()),
        WalletCommands::Assets => Ok("Wallet assets not yet implemented via RPC.".to_string()),
        WalletCommands::Keys => Ok("Wallet keys not yet implemented via RPC.".to_string()),
        WalletCommands::CreateAddress { count } => Ok(format!(
            "Create {} address(es) not yet implemented via RPC.",
            count
        )),
        WalletCommands::DeleteAddress { address } => Ok(format!(
            "Delete address {} not yet implemented via RPC.",
            address
        )),
        WalletCommands::ImportKey { key: _ } => {
            Ok("Import key not yet implemented via RPC.".to_string())
        }
        WalletCommands::ImportWatchOnly { address } => Ok(format!(
            "Import watch-only {} not yet implemented via RPC.",
            address
        )),
        WalletCommands::ExportKey { address, path } => Ok(format!(
            "Export key for {:?} to {:?} not yet implemented via RPC.",
            address, path
        )),
        WalletCommands::ChangePassword => {
            Ok("Change password not yet implemented via RPC.".to_string())
        }
        WalletCommands::Upgrade { path } => Ok(format!(
            "Upgrade wallet {} not yet implemented via RPC.",
            path
        )),
    }
}
