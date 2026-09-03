use super::{LocalNode, LocalNodeCommand, RelayInventory, RemoteNodeSnapshot};
use crate::network::p2p::{ChannelsConfig, PeerCommand};
use crate::runtime::{ActorRef, ActorRuntimeError, ActorRuntimeResult};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::oneshot;

/// Typed facade for sending commands to the local node actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalNodeHandle {
    raw: ActorRef,
}

impl LocalNodeHandle {
    /// Wraps a raw actor reference with the local node command boundary.
    pub fn new(raw: ActorRef) -> Self {
        Self { raw }
    }

    /// Returns the raw actor reference for watcher/runtime integration.
    pub fn raw_ref(&self) -> &ActorRef {
        &self.raw
    }

    /// Sends a local node command without an actor sender.
    pub fn tell(&self, command: LocalNodeCommand) -> ActorRuntimeResult<()> {
        self.raw.tell(command)
    }

    /// Sends a local node command with an optional actor sender.
    pub fn tell_from(
        &self,
        command: LocalNodeCommand,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()> {
        self.raw.tell_from(command, sender)
    }

    fn tell_peer(&self, command: PeerCommand) -> ActorRuntimeResult<()> {
        self.raw.tell(command)
    }

    async fn ask<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<T>) -> LocalNodeCommand,
    ) -> ActorRuntimeResult<T>
    where
        T: Send + 'static,
    {
        let (reply, response) = oneshot::channel();
        self.tell(build(reply))?;
        response
            .await
            .map_err(|_| ActorRuntimeError::system("local node actor dropped response"))
    }

    /// Starts or reconfigures local node networking.
    pub fn configure(&self, config: ChannelsConfig) -> ActorRuntimeResult<()> {
        self.tell_peer(PeerCommand::Configure { config })
    }

    /// Records a connected peer snapshot.
    pub fn add_peer(
        &self,
        remote_address: SocketAddr,
        listener_tcp_port: Option<u16>,
        version: u32,
        services: u64,
        last_block_index: u32,
    ) -> ActorRuntimeResult<()> {
        self.tell(LocalNodeCommand::AddPeer {
            remote_address,
            listener_tcp_port,
            version,
            services,
            last_block_index,
        })
    }

    /// Adds endpoints to the unconnected peer queue.
    pub fn add_unconnected_peers(&self, endpoints: Vec<SocketAddr>) -> ActorRuntimeResult<()> {
        self.tell(LocalNodeCommand::AddUnconnectedPeers { endpoints })
    }

    /// Updates the last reported block height for a peer.
    pub fn update_peer_height(
        &self,
        remote_address: SocketAddr,
        last_block_index: u32,
    ) -> ActorRuntimeResult<()> {
        self.tell(LocalNodeCommand::UpdatePeerHeight {
            remote_address,
            last_block_index,
        })
    }

    /// Removes a connected peer.
    pub async fn remove_peer(&self, remote_address: SocketAddr) -> ActorRuntimeResult<bool> {
        self.ask(|reply| LocalNodeCommand::RemovePeer {
            remote_address,
            reply,
        })
        .await
    }

    /// Returns the connected peer count.
    pub async fn peer_count(&self) -> ActorRuntimeResult<usize> {
        self.ask(|reply| LocalNodeCommand::PeerCount { reply })
            .await
    }

    /// Returns the queued unconnected peer count.
    pub async fn unconnected_count(&self) -> ActorRuntimeResult<usize> {
        self.ask(|reply| LocalNodeCommand::UnconnectedCount { reply })
            .await
    }

    /// Returns queued unconnected peer endpoints.
    pub async fn unconnected_peers(&self) -> ActorRuntimeResult<Vec<SocketAddr>> {
        self.ask(|reply| LocalNodeCommand::GetUnconnectedPeers { reply })
            .await
    }

    /// Returns connected peer endpoints.
    pub async fn peers(&self) -> ActorRuntimeResult<Vec<SocketAddr>> {
        self.ask(|reply| LocalNodeCommand::GetPeers { reply }).await
    }

    /// Returns detailed connected peer snapshots.
    pub async fn remote_node_snapshots(&self) -> ActorRuntimeResult<Vec<RemoteNodeSnapshot>> {
        self.ask(|reply| LocalNodeCommand::GetRemoteNodes { reply })
            .await
    }

    /// Fetches the shared local node state.
    pub async fn local_node_state(&self) -> ActorRuntimeResult<Arc<LocalNode>> {
        self.ask(|reply| LocalNodeCommand::GetInstance { reply })
            .await
    }

    /// Relays inventory to peers without an actor sender.
    pub fn relay_directly(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> ActorRuntimeResult<()> {
        self.tell(LocalNodeCommand::RelayDirectly {
            inventory,
            block_index,
        })
    }

    /// Relays inventory to peers with an actor sender.
    pub fn relay_directly_from(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()> {
        self.tell_from(
            LocalNodeCommand::RelayDirectly {
                inventory,
                block_index,
            },
            sender,
        )
    }

    /// Sends inventory directly to peers.
    pub fn send_directly(
        &self,
        inventory: RelayInventory,
        block_index: Option<u32>,
    ) -> ActorRuntimeResult<()> {
        self.tell(LocalNodeCommand::SendDirectly {
            inventory,
            block_index,
        })
    }
}

impl From<ActorRef> for LocalNodeHandle {
    fn from(raw: ActorRef) -> Self {
        Self::new(raw)
    }
}
