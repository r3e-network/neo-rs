use std::sync::Arc;

use futures::SinkExt;
use tracing::trace;

use super::super::{InboundInventory, PeerFramed};
use super::{CloseReason, PeerSession};
use crate::MessageCommand;
use crate::wire::Message;
use neo_io::{MemoryReader, Serializable};
use neo_payloads::p2p_payloads::{
    AddrPayload, GetBlockByIndexPayload, GetBlocksPayload, InvPayload, NetworkAddressWithTime,
    NodeCapability, PingPayload,
};
use neo_payloads::{Block, ExtensiblePayload, HeadersPayload, Transaction};
use neo_primitives::{InventoryType, UInt256};

impl PeerSession {
    /// Dispatch one inbound frame, enforcing the C#
    /// `RemoteNode.ProtocolHandler.OnMessage` handshake ordering.
    pub(super) async fn on_message(
        &mut self,
        framed: &mut PeerFramed,
        message: Message,
    ) -> Result<(), CloseReason> {
        if self.peer_version.is_none() {
            if message.command != MessageCommand::Version {
                return Err(CloseReason::ProtocolViolation(format!(
                    "expected version, received {:?}",
                    message.command
                )));
            }
            return self.on_version_message(framed, &message.payload_raw).await;
        }
        if !self.verack_received {
            if message.command != MessageCommand::Verack {
                return Err(CloseReason::ProtocolViolation(format!(
                    "expected verack, received {:?}",
                    message.command
                )));
            }
            return self.on_verack_message(framed).await;
        }
        match message.command {
            // C# treats a repeated version/verack after the handshake
            // as a ProtocolViolationException.
            MessageCommand::Version | MessageCommand::Verack => {
                Err(CloseReason::ProtocolViolation(format!(
                    "unexpected {:?} after handshake",
                    message.command
                )))
            }
            // C# `RemoteNode.ProtocolHandler.OnPingMessageReceived`: record
            // the peer's advertised height and reply with our own ping
            // payload as a pong. The inbound frame already reset the idle
            // deadline in the drive loop (C# `Connection` 60 s timer reset).
            MessageCommand::Ping => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = PingPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid ping payload: {err}"))
                })?;
                self.peer_last_block_index = payload.last_block_index;
                let pong =
                    PingPayload::create_with_nonce(self.identity.block_height(), payload.nonce);
                let message = Message::create(
                    MessageCommand::Pong,
                    Some(&pong),
                    self.peer_allows_compression,
                )
                .map_err(|err| CloseReason::Transport(format!("encode pong: {err}")))?;
                framed
                    .send(message)
                    .await
                    .map_err(|err| CloseReason::Transport(format!("send pong: {err}")))?;
                Ok(())
            }
            // C# `OnPongMessageReceived`: refresh the peer's reported height.
            MessageCommand::Pong => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = PingPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid pong payload: {err}"))
                })?;
                self.peer_last_block_index = payload.last_block_index;
                Ok(())
            }
            // C# `OnInventoryReceived` for a relayed `Block`: decode and
            // forward to the ledger via the inbound-inventory sink. The
            // blockchain service applies the C# `Blockchain.OnNewBlock`
            // sequencing (persist when it is the next block, park when
            // ahead, drop when already known).
            MessageCommand::Block => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let block = Block::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid block payload: {err}"))
                })?;
                if let Some(tx) = &self.inbound_tx {
                    let _ = tx.send(InboundInventory::Block(Arc::new(block))).await;
                }
                Ok(())
            }
            // C# `OnGetBlockByIndexMessageReceived`: serve the requested
            // blocks `[IndexStart, IndexStart + min(Count, 500))` from the
            // local ledger as `block` frames, stopping at the first block we
            // do not hold (matching C#'s `GetBlock == null` break).
            MessageCommand::GetBlockByIndex => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = GetBlockByIndexPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!(
                        "invalid getblockbyindex payload: {err}"
                    ))
                })?;
                if let Some(source) = self.block_source.clone() {
                    // C# caps a response at `InvPayload.MaxHashesCount` (500);
                    // `Count == -1` means "as many as available".
                    let count = if payload.count < 0 {
                        500u32
                    } else {
                        (payload.count as u32).min(500)
                    };
                    let end = payload.index_start.saturating_add(count);
                    for index in payload.index_start..end {
                        let Some(block) = source.block_by_index(index) else {
                            break;
                        };
                        let served = Message::create(
                            MessageCommand::Block,
                            Some(&block),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| {
                            CloseReason::Transport(format!("encode served block: {err}"))
                        })?;
                        framed.send(served).await.map_err(|err| {
                            CloseReason::Transport(format!("send served block: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnGetBlocksMessageReceived`: starting just after the block
            // named by `hash_start`, reply with an `Inv` of up to `count`
            // (default/-1 => MaxHashesCount 500) subsequent block hashes from
            // the local chain. The legacy hash-based sync request, kept for
            // compatibility alongside `GetBlockByIndex`.
            MessageCommand::GetBlocks => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = GetBlocksPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid getblocks payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    if let Some(start_index) = source.block_index_by_hash(&payload.hash_start) {
                        let count = if payload.count < 0 {
                            neo_payloads::inv_payload::MAX_HASHES_COUNT as u32
                        } else {
                            (payload.count as u32)
                                .min(neo_payloads::inv_payload::MAX_HASHES_COUNT as u32)
                        };
                        let mut hashes = Vec::new();
                        for offset in 1..=count {
                            match source.block_hash_by_index(start_index.saturating_add(offset)) {
                                Some(hash) => hashes.push(hash),
                                None => break,
                            }
                        }
                        for group in InvPayload::create_group(InventoryType::Block, hashes) {
                            let inv = Message::create(
                                MessageCommand::Inv,
                                Some(&group),
                                self.peer_allows_compression,
                            )
                            .map_err(|err| {
                                CloseReason::Transport(format!("encode getblocks inv: {err}"))
                            })?;
                            framed.send(inv).await.map_err(|err| {
                                CloseReason::Transport(format!("send getblocks inv: {err}"))
                            })?;
                        }
                    }
                }
                Ok(())
            }
            // C# `OnGetHeadersMessageReceived`: serve up to 2000 headers from
            // `IndexStart` as a single `headers` frame (HeadersPayload).
            MessageCommand::GetHeaders => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = GetBlockByIndexPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid getheaders payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    // C# `HeadersPayload.MaxHeadersCount` is 2000.
                    let count = if payload.count < 0 {
                        2000u32
                    } else {
                        (payload.count as u32).min(2000)
                    };
                    let mut headers = Vec::new();
                    for index in payload.index_start..payload.index_start.saturating_add(count) {
                        match source.header_by_index(index) {
                            Some(header) => headers.push(header),
                            None => break,
                        }
                    }
                    if !headers.is_empty() {
                        let hp = HeadersPayload::create(headers);
                        let served = Message::create(
                            MessageCommand::Headers,
                            Some(&hp),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| CloseReason::Transport(format!("encode headers: {err}")))?;
                        framed.send(served).await.map_err(|err| {
                            CloseReason::Transport(format!("send headers: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnGetDataMessageReceived`: for each requested inventory hash,
            // serve the matching block / transaction / extensible frame.
            MessageCommand::GetData => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = InvPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid getdata payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    let mut not_found = Vec::new();
                    for hash in &payload.hashes {
                        match payload.inventory_type {
                            InventoryType::Block => {
                                if let Some(block) = source.block_by_hash(hash) {
                                    let served = Message::create(
                                        MessageCommand::Block,
                                        Some(&block),
                                        self.peer_allows_compression,
                                    )
                                    .map_err(|err| {
                                        CloseReason::Transport(format!(
                                            "encode getdata block: {err}"
                                        ))
                                    })?;
                                    framed.send(served).await.map_err(|err| {
                                        CloseReason::Transport(format!("send getdata block: {err}"))
                                    })?;
                                } else {
                                    not_found.push(*hash);
                                }
                            }
                            InventoryType::Transaction => {
                                if let Some(tx) = source.transaction_by_hash(hash) {
                                    let served = Message::create(
                                        MessageCommand::Transaction,
                                        Some(&tx),
                                        self.peer_allows_compression,
                                    )
                                    .map_err(|err| {
                                        CloseReason::Transport(format!("encode getdata tx: {err}"))
                                    })?;
                                    framed.send(served).await.map_err(|err| {
                                        CloseReason::Transport(format!("send getdata tx: {err}"))
                                    })?;
                                } else {
                                    not_found.push(*hash);
                                }
                            }
                            InventoryType::Extensible => {
                                if let Some(payload) = source.extensible_by_hash(hash) {
                                    let served = Message::create(
                                        MessageCommand::Extensible,
                                        Some(&payload),
                                        self.peer_allows_compression,
                                    )
                                    .map_err(|err| {
                                        CloseReason::Transport(format!(
                                            "encode getdata extensible: {err}"
                                        ))
                                    })?;
                                    framed.send(served).await.map_err(|err| {
                                        CloseReason::Transport(format!(
                                            "send getdata extensible: {err}"
                                        ))
                                    })?;
                                }
                            }
                        }
                    }
                    for group in InvPayload::create_group(payload.inventory_type, not_found) {
                        let not_found = Message::create(
                            MessageCommand::NotFound,
                            Some(&group),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| {
                            CloseReason::Transport(format!("encode getdata notfound: {err}"))
                        })?;
                        framed.send(not_found).await.map_err(|err| {
                            CloseReason::Transport(format!("send getdata notfound: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnInventoryReceived` for a relayed `Transaction`: decode
            // and forward for mempool admission.
            MessageCommand::Transaction => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let transaction = Transaction::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid transaction payload: {err}"))
                })?;
                if let Some(tx) = &self.inbound_tx {
                    let _ = tx
                        .send(InboundInventory::Transaction(Arc::new(transaction)))
                        .await;
                }
                Ok(())
            }
            // C# `OnInventoryReceived` for an `ExtensiblePayload`: decode and
            // forward to the ledger/consensus relay (dBFT + state-root votes).
            MessageCommand::Extensible => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = ExtensiblePayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid extensible payload: {err}"))
                })?;
                if let Some(tx) = &self.inbound_tx {
                    let _ = tx
                        .send(InboundInventory::Extensible(Arc::new(payload)))
                        .await;
                }
                Ok(())
            }
            // C# `RemoteNode.OnInvMessageReceived`: a peer announces inventory;
            // pull the items we don't already hold via `GetData`.
            MessageCommand::Inv => {
                let mut reader = MemoryReader::new(&message.payload_raw);
                let payload = InvPayload::deserialize(&mut reader).map_err(|err| {
                    CloseReason::ProtocolViolation(format!("invalid inv payload: {err}"))
                })?;
                if let Some(source) = self.block_source.clone() {
                    let unknown: Vec<UInt256> = payload
                        .hashes
                        .iter()
                        .copied()
                        .filter(|hash| match payload.inventory_type {
                            InventoryType::Block => !source.contains_block(hash),
                            InventoryType::Transaction => !source.contains_transaction(hash),
                            // Neo N3 pulls ExtensiblePayload inventory by hash;
                            // consensus and state-service payloads are both
                            // carried by MessageCommand::Extensible.
                            InventoryType::Extensible => true,
                        })
                        .collect();
                    for group in InvPayload::create_group(payload.inventory_type, unknown) {
                        let getdata = Message::create(
                            MessageCommand::GetData,
                            Some(&group),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| CloseReason::Transport(format!("encode getdata: {err}")))?;
                        framed.send(getdata).await.map_err(|err| {
                            CloseReason::Transport(format!("send getdata: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `RemoteNode.OnMemPoolMessageReceived`: reply with `Inv`
            // announcements of every verified mempool transaction.
            MessageCommand::Mempool => {
                if let Some(source) = self.block_source.clone() {
                    let hashes = source.mempool_transaction_hashes();
                    for group in InvPayload::create_group(InventoryType::Transaction, hashes) {
                        let inv = Message::create(
                            MessageCommand::Inv,
                            Some(&group),
                            self.peer_allows_compression,
                        )
                        .map_err(|err| {
                            CloseReason::Transport(format!("encode mempool inv: {err}"))
                        })?;
                        framed.send(inv).await.map_err(|err| {
                            CloseReason::Transport(format!("send mempool inv: {err}"))
                        })?;
                    }
                }
                Ok(())
            }
            // C# `OnGetAddrMessageReceived`: gossip up to `MAX_COUNT_TO_SEND`
            // connected peers' advertised listener endpoints (deduplicated,
            // excluding the requester) as a single `Addr` frame.
            MessageCommand::GetAddr => {
                let addrs = self.registry.listener_addresses(
                    self.peer_id,
                    neo_payloads::addr_payload::MAX_COUNT_TO_SEND,
                );
                if !addrs.is_empty() {
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as u32)
                        .unwrap_or(0);
                    let entries: Vec<NetworkAddressWithTime> = addrs
                        .into_iter()
                        .map(|addr| {
                            NetworkAddressWithTime::new(
                                timestamp,
                                addr.ip(),
                                vec![NodeCapability::TcpServer { port: addr.port() }],
                            )
                        })
                        .collect();
                    let payload = AddrPayload::create(entries);
                    let served = Message::create(
                        MessageCommand::Addr,
                        Some(&payload),
                        self.peer_allows_compression,
                    )
                    .map_err(|err| CloseReason::Transport(format!("encode addr: {err}")))?;
                    framed
                        .send(served)
                        .await
                        .map_err(|err| CloseReason::Transport(format!("send addr: {err}")))?;
                }
                Ok(())
            }
            other => {
                // Genuine no-ops for this node profile. C# default arm:
                // Alert/MerkleBlock/NotFound/Reject/FilterAdd/FilterClear/
                // FilterLoad. `Addr` is also ignored here, matching C#
                // `OnAddrMessageReceived` (`if (!sent) return;`): this node
                // never sends `GetAddr`, so unsolicited `Addr` is dropped.
                trace!(
                    target: "neo_network",
                    peer_id = %self.peer_id,
                    command = ?other,
                    payload_len = message.payload_raw.len(),
                    "no-op post-handshake message for this node profile"
                );
                Ok(())
            }
        }
    }
}
