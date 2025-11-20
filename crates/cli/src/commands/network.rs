use super::CommandResult;
use crate::console_service::ConsoleHelper;
use akka::{Actor, ActorContext, ActorRef, ActorResult, Props};
use anyhow::{anyhow, bail};
use async_trait::async_trait;
use chrono::{Local, TimeZone};
use neo_core::{
    ledger::{RelayResult, VerifyResult},
    neo_system::{NeoSystem, TransactionRouterMessage},
    network::p2p::{
        capabilities::tcp_server,
        message::Message,
        message_command::MessageCommand,
        payloads::inventory_type::InventoryType,
        payloads::{
            addr_payload::AddrPayload, get_block_by_index_payload::GetBlockByIndexPayload,
            get_blocks_payload::GetBlocksPayload, inv_payload::InvPayload,
            network_address_with_time::NetworkAddressWithTime, ping_payload::PingPayload,
            transaction::Transaction,
        },
    },
    smart_contract::contract_parameters_context::ContractParametersContext,
    UInt256,
};
use std::{
    any::Any,
    fs,
    net::IpAddr,
    path::Path,
    sync::{mpsc::channel, Arc, Mutex as StdMutex},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// Peer/network commands (`MainService.Network`).
pub struct NetworkCommands {
    system: Arc<NeoSystem>,
}

impl NetworkCommands {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        Self { system }
    }

    /// Lists connected peers (parity with `show node` info).
    pub fn show_nodes(&self) -> CommandResult {
        let handle = tokio::runtime::Handle::current();
        let peers = handle
            .block_on(self.system.remote_node_snapshots())
            .map_err(|err| anyhow!("failed to retrieve peers: {}", err))?;
        let unconnected = handle
            .block_on(self.system.unconnected_peers())
            .unwrap_or_default();

        if peers.is_empty() {
            ConsoleHelper::info(["No connected peers."]);
        } else {
            ConsoleHelper::info(["Connected peers:"]);
            for peer in peers {
                let ts = Local
                    .timestamp_opt(peer.timestamp as i64, 0)
                    .single()
                    .unwrap_or_else(|| Local.timestamp_opt(0, 0).earliest().unwrap());
                ConsoleHelper::info([
                    " - ",
                    &format!(
                        "{} (listen {}; height {}; version {}; seen {})",
                        peer.remote_address,
                        peer.listen_tcp_port,
                        peer.last_block_index,
                        peer.version,
                        ts
                    ),
                ]);
            }
        }

        if !unconnected.is_empty() {
            ConsoleHelper::info(["", "Unconnected peers:"]);
            for endpoint in unconnected {
                ConsoleHelper::info([" - ", &endpoint.to_string()]);
            }
        }
        Ok(())
    }

    /// Relays a signed transaction from a JSON context payload.
    pub fn relay(&self, input: &str) -> CommandResult {
        let payload = self.read_context_input(input)?;
        let store_cache = self.system.store_cache();
        let snapshot_arc = Arc::new(store_cache.data_cache().clone());
        let (context, mut tx) =
            ContractParametersContext::parse_transaction_context(&payload, snapshot_arc)
                .map_err(|err| anyhow!("failed to parse context: {}", err))?;
        if !context.completed() {
            bail!("The signature is incomplete.");
        }
        let witnesses = context
            .get_witnesses()
            .ok_or_else(|| anyhow!("failed to extract witnesses"))?;
        tx.set_witnesses(witnesses);
        self.relay_transaction(tx.clone())?;
        ConsoleHelper::info([
            "Data relay success, the hash is shown as follows: ",
            &tx.hash().to_string(),
        ]);
        Ok(())
    }

    /// Broadcasts a simple ping using the current block height.
    pub fn broadcast_ping(&self) -> CommandResult {
        let payload = PingPayload::create(self.system.current_block_index());
        self.local_node_message(MessageCommand::Ping, payload)
    }

    /// Broadcasts a getblocks request starting from the given hash.
    pub fn broadcast_getblocks(&self, hash: &UInt256) -> CommandResult {
        let payload = GetBlocksPayload::create(*hash, -1);
        self.local_node_message(MessageCommand::GetBlocks, payload)
    }

    /// Broadcasts a getheaders request starting from the given index.
    pub fn broadcast_getheaders(&self, start: u32) -> CommandResult {
        let payload = GetBlockByIndexPayload::create(start, -1);
        self.local_node_message(MessageCommand::GetHeaders, payload)
    }

    /// Broadcasts a getdata/inv request with the given inventory type and hashes.
    pub fn broadcast_inv(&self, inv_type: InventoryType, hashes: Vec<UInt256>) -> CommandResult {
        let payload = InvPayload::create(inv_type, &hashes);
        self.local_node_message(MessageCommand::Inv, payload)
    }

    pub fn broadcast_getdata(
        &self,
        inv_type: InventoryType,
        hashes: Vec<UInt256>,
    ) -> CommandResult {
        let payload = InvPayload::create(inv_type, &hashes);
        self.local_node_message(MessageCommand::GetData, payload)
    }

    /// Broadcasts a block payload by hash or height.
    pub fn broadcast_block(&self, index_or_hash: &str) -> CommandResult {
        let block = if let Ok(index) = index_or_hash.parse::<u32>() {
            let hash = self
                .system
                .block_hash_at(index)
                .ok_or_else(|| anyhow!("Block {} not found", index))?;
            self.system
                .context()
                .try_get_block(&hash)
                .ok_or_else(|| anyhow!("Block {} not found", index))?
        } else {
            let hash = index_or_hash
                .parse::<UInt256>()
                .map_err(|_| anyhow!("invalid hash"))?;
            self.system
                .context()
                .try_get_block(&hash)
                .ok_or_else(|| anyhow!("Block {} not found", index_or_hash))?
        };
        self.local_node_message(MessageCommand::Block, block)
    }

    /// Broadcasts a transaction from the mempool if available.
    pub fn broadcast_transaction(&self, hash: &UInt256) -> CommandResult {
        let tx = self
            .system
            .context()
            .try_get_transaction(hash)
            .ok_or_else(|| anyhow!("transaction {} not found locally", hash))?;
        self.local_node_message(MessageCommand::Transaction, tx)
    }

    /// Broadcasts a single addr entry for diagnostics.
    pub fn broadcast_addr(&self, address: NetworkAddressWithTime) -> CommandResult {
        let payload = AddrPayload::create(vec![address]);
        self.local_node_message(MessageCommand::Addr, payload)
    }

    pub fn broadcast_addr_host(&self, host: &str, port: u16) -> CommandResult {
        let ip: IpAddr = host
            .parse()
            .map_err(|_| anyhow!("invalid IP address '{}'", host))?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        let address = NetworkAddressWithTime::new(ts, ip, vec![tcp_server(port)]);
        self.broadcast_addr(address)
    }

    fn local_node_message<P>(&self, command: MessageCommand, payload: P) -> CommandResult
    where
        P: neo_core::neo_io::Serializable + Send + 'static,
    {
        let message = Message::create(command, Some(&payload), false)
            .map_err(|err| anyhow!("failed to build message: {}", err))?;
        self.system
            .local_node_actor()
            .tell(message)
            .map_err(|err| anyhow!("failed to broadcast message: {}", err))
    }

    fn read_context_input(&self, input: &str) -> Result<String, anyhow::Error> {
        let path = Path::new(input);
        if path.exists() && path.is_file() {
            fs::read_to_string(path)
                .map_err(|err| anyhow!("failed to read {}: {}", path.display(), err))
        } else {
            Ok(input.to_string())
        }
    }

    fn relay_transaction(&self, tx: Transaction) -> CommandResult {
        let cloned = tx.clone();
        let result = self.with_relay_responder(|sender| {
            self.system
                .tx_router_actor()
                .tell_from(
                    TransactionRouterMessage::Preverify {
                        transaction: cloned,
                        relay: true,
                    },
                    Some(sender),
                )
                .map_err(|err| anyhow!("failed to submit transaction: {}", err))
        })?;
        self.map_relay_result(result)
    }

    fn with_relay_responder<F>(&self, send: F) -> Result<RelayResult, anyhow::Error>
    where
        F: FnOnce(ActorRef) -> Result<(), anyhow::Error>,
    {
        struct RelayResponder {
            tx: Arc<StdMutex<Option<std::sync::mpsc::Sender<RelayResult>>>>,
        }

        #[async_trait]
        impl Actor for RelayResponder {
            async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
                Ok(())
            }

            async fn handle(
                &mut self,
                msg: Box<dyn Any + Send>,
                _ctx: &mut ActorContext,
            ) -> ActorResult {
                if let Ok(result) = msg.downcast::<RelayResult>() {
                    if let Some(sender) = self.tx.lock().unwrap().take() {
                        let _ = sender.send(*result);
                    }
                }
                Ok(())
            }
        }

        let (tx, rx) = channel();
        let responder = RelayResponder {
            tx: Arc::new(StdMutex::new(Some(tx))),
        };
        let actor_ref = self
            .system
            .actor_system()
            .actor_of(
                Props::new(move || RelayResponder {
                    tx: Arc::clone(&responder.tx),
                }),
                format!("network_relay_responder_{}", Uuid::new_v4()),
            )
            .map_err(|err| anyhow!("failed to create relay responder: {}", err))?;

        send(actor_ref.clone())?;

        let result = rx
            .recv()
            .map_err(|err| anyhow!("failed to receive relay result: {}", err))?;
        Ok(result)
    }

    fn map_relay_result(&self, result: RelayResult) -> CommandResult {
        match result.result {
            VerifyResult::Succeed => {
                ConsoleHelper::info([
                    "Transaction relayed successfully. Hash: ",
                    &result.hash.to_string(),
                ]);
                Ok(())
            }
            VerifyResult::AlreadyExists => {
                bail!("Transaction already exists on the blockchain.")
            }
            VerifyResult::AlreadyInPool => bail!("Transaction already exists in the mempool."),
            VerifyResult::OutOfMemory => bail!("Mempool capacity reached."),
            VerifyResult::InvalidScript => bail!("Transaction script is invalid."),
            VerifyResult::InvalidAttribute => bail!("Transaction contains invalid attributes."),
            VerifyResult::InvalidSignature => bail!("Transaction contains invalid signatures."),
            VerifyResult::OverSize => bail!("Transaction exceeds the allowed size."),
            VerifyResult::Expired => bail!("Transaction has already expired."),
            VerifyResult::InsufficientFunds => {
                bail!("Insufficient funds for the requested transfer.")
            }
            VerifyResult::PolicyFail => bail!("Transaction rejected by policy."),
            VerifyResult::UnableToVerify => bail!("Transaction cannot be verified at this time."),
            VerifyResult::Invalid => bail!("Transaction verification failed."),
            VerifyResult::HasConflicts => {
                bail!("Transaction conflicts with an existing mempool entry.")
            }
            VerifyResult::Unknown => {
                bail!("Transaction verification failed for an unknown reason.")
            }
        }
    }
}
