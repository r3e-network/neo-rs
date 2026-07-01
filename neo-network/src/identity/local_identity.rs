//! Local node identity advertised during the P2P version handshake.
//!
//! Mirrors the identity surface of C# `LocalNode`: the static `Nonce`
//! and `UserAgent` (`LocalNode.cs:74-84`), the listener port recorded
//! by `Peer.OnStart`, and `LocalNode.GetNodeCapabilities()` which
//! assembles the capability list embedded in every outbound
//! `VersionPayload` (`LocalNode.cs:259-275`).

use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};

use neo_payloads::p2p_payloads::{NodeCapability, VersionPayload};

/// Identity of the local node as advertised to remote peers in the
/// version handshake.
///
/// One instance is shared (via `Arc`) between the
/// [`crate::local_node::LocalNodeService`] and every
/// [`crate::remote_node::RemoteNodeService`] it spawns, so each
/// per-peer task can assemble the outbound [`VersionPayload`] without
/// a service round-trip. The nonce and user agent are sourced from
/// the [`crate::handle::NetworkHandle`] family created alongside the
/// local node service, keeping the values the RPC layer reports
/// (`getversion`) byte-identical to the values sent on the wire.
#[derive(Debug)]
pub struct LocalIdentity {
    /// Network magic (C# `ProtocolSettings.Network`), echoed in the
    /// version payload and validated against the peer's version.
    network: u32,
    /// Random identity nonce (C# `LocalNode.Nonce`). A peer whose
    /// version carries this exact nonce is ourselves and is dropped.
    nonce: u32,
    /// Node software identifier (C# `LocalNode.UserAgent`).
    user_agent: String,
    /// Whether this node accepts compressed inbound frames. When
    /// `false` the version payload advertises the
    /// `DisableCompression` capability (C# `LocalNode.GetNodeCapabilities`).
    enable_compression: bool,
    /// TCP listener port, recorded once the listener is bound
    /// (C# `Peer.ListenerTcpPort`). `0` while not listening, in which
    /// case the `TcpServer` capability is omitted from the version
    /// payload — exactly how C# treats a non-server node.
    listen_port: AtomicU16,
    /// Current local block height (C# `NativeContract.Ledger.CurrentIndex`),
    /// advertised in the `FullNode` capability of every outbound version
    /// payload and in `ping`/`pong` keepalives. `0` until the ledger
    /// pipeline records progress via [`LocalIdentity::set_block_height`].
    block_height: AtomicU32,
}

impl LocalIdentity {
    /// Build a fresh identity. `listen_port` starts at `0` and is
    /// recorded via [`LocalIdentity::set_listen_port`] once the
    /// listener is bound.
    pub fn new(network: u32, nonce: u32, user_agent: String, enable_compression: bool) -> Self {
        Self {
            network,
            nonce,
            user_agent,
            enable_compression,
            listen_port: AtomicU16::new(0),
            block_height: AtomicU32::new(0),
        }
    }

    /// Network magic this node speaks.
    pub fn network(&self) -> u32 {
        self.network
    }

    /// Random identity nonce of this node instance.
    pub fn nonce(&self) -> u32 {
        self.nonce
    }

    /// Node software identifier.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Record the bound TCP listener port so subsequent version
    /// payloads advertise the `TcpServer` capability.
    pub fn set_listen_port(&self, port: u16) {
        self.listen_port.store(port, Ordering::Relaxed);
    }

    /// Currently advertised TCP listener port (`0` when not listening).
    pub fn listen_port(&self) -> u16 {
        self.listen_port.load(Ordering::Relaxed)
    }

    /// Record the current local block height so subsequent version
    /// payloads and `ping`/`pong` keepalives advertise it (C# reads
    /// `NativeContract.Ledger.CurrentIndex` live; the ledger pipeline
    /// drives this seam as blocks persist).
    pub fn set_block_height(&self, height: u32) {
        self.block_height.store(height, Ordering::Relaxed);
    }

    /// Currently advertised local block height.
    pub fn block_height(&self) -> u32 {
        self.block_height.load(Ordering::Relaxed)
    }

    /// Capability list for the outbound version payload, mirroring
    /// C# `LocalNode.GetNodeCapabilities`:
    ///
    /// - `FullNode` with the current block height (C#
    ///   `NativeContract.Ledger.CurrentIndex`), sourced from
    ///   [`LocalIdentity::block_height`] — `0` until the ledger pipeline
    ///   records progress via [`LocalIdentity::set_block_height`].
    /// - `ArchivalNode` (C# always advertises it in v3.9.1).
    /// - `DisableCompression` when compression is disabled.
    /// - `TcpServer { port }` when the listener is bound.
    pub fn capabilities(&self) -> Vec<NodeCapability> {
        let mut capabilities = vec![
            NodeCapability::full_node(self.block_height()),
            NodeCapability::archival_node(),
        ];
        if !self.enable_compression {
            capabilities.push(NodeCapability::disable_compression());
        }
        let port = self.listen_port();
        if port > 0 {
            capabilities.push(NodeCapability::tcp_server(port));
        }
        capabilities
    }

    /// Assemble the outbound [`VersionPayload`] (C#
    /// `RemoteNode.OnStartProtocol` → `VersionPayload.Create`).
    pub fn version_payload(&self) -> VersionPayload {
        VersionPayload::create(
            self.network,
            self.nonce,
            self.user_agent.clone(),
            self.capabilities(),
        )
    }
}

#[cfg(test)]
#[path = "../tests/identity/local_identity.rs"]
mod tests;
