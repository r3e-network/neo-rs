//! Local P2P node actor: peer management, listener, and routing glue.
//!
//! This module implements the local node component of the Neo N3 P2P network,
//! mirroring the C# `Neo.Network.P2P.LocalNode` class.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      LocalNode (State)                       │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ Peer Registry│  │ Remote Nodes │  │ Broadcast History│  │
//! │  │ (connected)  │  │ (actors)     │  │ (diagnostics)    │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ Seed List    │  │ Capabilities │  │ Pending Connects │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                   LocalNodeActor (Behavior)                  │
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ TCP Listener │  │ Peer Timer   │  │ Connection Mgmt  │  │
//! │  │ (inbound)    │  │ (heartbeat)  │  │ (outbound)       │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Types
//!
//! - [`LocalNode`]: Shared state for the local P2P node (thread-safe)
//! - [`LocalNodeActor`]: Actor handling peer lifecycle and message routing
//! - [`LocalNodeCommand`]: Messages accepted by the actor
//! - [`RelayInventory`]: Block, transaction, or extensible payload for relay
//! - [`BroadcastEvent`]: Diagnostic record of broadcast operations
//!
//! # Connection Flow
//!
//! 1. **Outbound**: `PeerCommand::Connect` → TCP connect → handshake → register
//! 2. **Inbound**: TCP accept → `InboundTcpAccepted` → handshake → register
//! 3. **Handshake**: Exchange `VersionPayload` → validate → `ConnectionEstablished`
//!
//! # Thread Safety
//!
//! `LocalNode` uses `RwLock` for all mutable state, allowing concurrent reads
//! and exclusive writes. The actor model ensures message processing is sequential.
//
// Copyright (C) 2015-2025 The Neo Project.
//
// local_node/mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    capabilities::NodeCapability,
    channels_config::ChannelsConfig,
    peer::{PeerCommand, PeerState, PeerTimer, MAX_COUNT_FROM_SEED_LIST},
    remote_node::{RemoteNode, RemoteNodeCommand},
};
use crate::akka::{Actor, ActorContext, ActorRef, ActorResult, Props, Terminated};
use crate::neo_io::{BinaryWriter, Serializable};
use crate::network::p2p::payloads::{
    addr_payload::MAX_COUNT_TO_SEND, block::Block, extensible_payload::ExtensiblePayload,
    network_address_with_time::NetworkAddressWithTime, transaction::Transaction, InventoryType,
    VersionPayload,
};
use crate::network::p2p::{NetworkMessage, ProtocolMessage};
use crate::services::PeerManagerService;
use crate::{neo_system::NeoSystemContext, protocol_settings::ProtocolSettings};
use async_trait::async_trait;
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use rand::{rngs::OsRng, seq::IteratorRandom, thread_rng, RngCore};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, TcpListener as StdTcpListener};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::task::JoinHandle;
use tokio::{
    net::{lookup_host, TcpListener, TcpStream},
    sync::{oneshot, Mutex},
};
use tracing::{debug, error, trace, warn};

/// The protocol version supported by this node implementation (matches C# LocalNode.ProtocolVersion).
/// Neo N3 uses protocol version 0 during handshake but validates network magic for compatibility.
/// Note: This constant is used for Version message payload, not the Neo3 protocol version indicator.
pub const PROTOCOL_VERSION: u32 = 0;

/// TCP connection timeout for outbound peer connections.
/// Prevents resource exhaustion from hanging connections.
pub const TCP_CONNECTION_TIMEOUT: Duration = Duration::from_secs(6);

mod actor;
mod actor_impl;
mod helpers;
mod state;
mod types;

// Re-exports
pub use actor::LocalNodeActor;
pub use state::LocalNode;
pub use types::{BroadcastEvent, LocalNodeCommand, RelayInventory, RemoteNodeSnapshot};

#[cfg(test)]
mod tests;
