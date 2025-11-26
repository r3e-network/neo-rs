//! Broadcast commands - broadcast network messages

use super::CommandResult;
use crate::BroadcastCommands;
use neo_rpc_client::RpcClient;

pub async fn execute(_client: &RpcClient, cmd: BroadcastCommands) -> CommandResult {
    // Broadcast commands typically require direct P2P network access
    // which is only available on neo-node

    match cmd {
        BroadcastCommands::Addr => {
            Ok("Broadcast addr requires direct node access. Use neo-node.".to_string())
        }
        BroadcastCommands::Block { block } => {
            Ok(format!("Broadcast block {} requires direct node access. Use neo-node.", block))
        }
        BroadcastCommands::GetBlocks { start } => {
            Ok(format!("Broadcast getblocks from {} requires direct node access. Use neo-node.", start))
        }
        BroadcastCommands::GetData { inv_type, hash } => {
            Ok(format!("Broadcast getdata {} {} requires direct node access. Use neo-node.", inv_type, hash))
        }
        BroadcastCommands::GetHeaders { start } => {
            Ok(format!("Broadcast getheaders from {} requires direct node access. Use neo-node.", start))
        }
        BroadcastCommands::Inv { inv_type, hash } => {
            Ok(format!("Broadcast inv {} {} requires direct node access. Use neo-node.", inv_type, hash))
        }
        BroadcastCommands::Transaction { hash } => {
            Ok(format!("Broadcast transaction {} requires direct node access. Use neo-node.", hash))
        }
        BroadcastCommands::Ping => {
            Ok("Broadcast ping requires direct node access. Use neo-node.".to_string())
        }
    }
}
