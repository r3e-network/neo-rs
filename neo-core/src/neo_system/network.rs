//! Networking-facing helpers for `NeoSystem`.
//!
//! This module hosts the P2P actor interactions and async helper methods to keep
//! `core.rs` focused on system construction and orchestration.

use std::net::SocketAddr;
use std::sync::Arc;

use super::helpers::to_core_error;
use super::NeoSystem;
use crate::error::CoreResult;
use crate::network::p2p::local_node::RelayInventory;
use crate::network::p2p::{ChannelsConfig, LocalNode, RemoteNodeSnapshot};

impl NeoSystem {
    /// Starts the local node actor with the supplied networking configuration.
    pub fn start_node(&self, config: ChannelsConfig) -> CoreResult<()> {
        self.local_node.configure(config).map_err(to_core_error)
    }

    /// Records a new peer within the local node actor.
    pub fn add_peer(
        &self,
        remote_address: SocketAddr,
        listener_tcp_port: Option<u16>,
        version: u32,
        services: u64,
        last_block_index: u32,
    ) -> CoreResult<()> {
        self.local_node
            .add_peer(
                remote_address,
                listener_tcp_port,
                version,
                services,
                last_block_index,
            )
            .map_err(to_core_error)
    }

    /// Adds endpoints to the unconnected peer queue (parity with C# `LocalNode.AddPeers`).
    pub fn add_unconnected_peers(&self, endpoints: Vec<SocketAddr>) -> CoreResult<()> {
        self.local_node
            .add_unconnected_peers(endpoints)
            .map_err(to_core_error)
    }

    /// Updates the last reported block height for the specified peer.
    pub fn update_peer_height(
        &self,
        remote_address: SocketAddr,
        last_block_index: u32,
    ) -> CoreResult<()> {
        self.local_node
            .update_peer_height(remote_address, last_block_index)
            .map_err(to_core_error)
    }

    /// Removes the peer and returns whether a record existed.
    pub async fn remove_peer(&self, remote_address: SocketAddr) -> CoreResult<bool> {
        self.local_node
            .remove_peer(remote_address)
            .await
            .map_err(to_core_error)
    }

    /// Returns the number of peers currently tracked by the local node actor.
    pub async fn peer_count(&self) -> CoreResult<usize> {
        self.local_node
            .peer_count()
            .await
            .map_err(to_core_error)
    }

    /// Returns the number of queued unconnected peers.
    pub async fn unconnected_count(&self) -> CoreResult<usize> {
        self.local_node
            .unconnected_count()
            .await
            .map_err(to_core_error)
    }

    /// Returns the queued unconnected peers.
    pub async fn unconnected_peers(&self) -> CoreResult<Vec<SocketAddr>> {
        self.local_node
            .unconnected_peers()
            .await
            .map_err(to_core_error)
    }

    /// Returns the socket addresses for each connected peer.
    pub async fn peers(&self) -> CoreResult<Vec<SocketAddr>> {
        self.local_node
            .peers()
            .await
            .map_err(to_core_error)
    }

    /// Returns detailed snapshots for the connected peers.
    pub async fn remote_node_snapshots(&self) -> CoreResult<Vec<RemoteNodeSnapshot>> {
        self.local_node
            .remote_node_snapshots()
            .await
            .map_err(to_core_error)
    }

    /// Returns the maximum reported block height among connected peers.
    pub async fn max_peer_block_height(&self) -> CoreResult<u32> {
        let snapshots = self.remote_node_snapshots().await?;
        Ok(snapshots
            .into_iter()
            .map(|snap| snap.last_block_index)
            .max()
            .unwrap_or(0))
    }

    /// Fetches the shared local node snapshot for advanced operations.
    pub async fn local_node_state(&self) -> CoreResult<Arc<LocalNode>> {
        self.local_node
            .local_node_state()
            .await
            .map_err(to_core_error)
    }

    /// Records a relay broadcast via the local node actor.
    pub fn relay_directly(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> CoreResult<()> {
        self.local_node
            .relay_directly(inventory, block_index)
            .map_err(to_core_error)
    }

    /// Records a direct send broadcast via the local node actor.
    pub fn send_directly(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> CoreResult<()> {
        self.local_node
            .send_directly(inventory, block_index)
            .map_err(to_core_error)
    }
}
