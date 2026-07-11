//! # neo-network::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-network`. This service crate owns P2P transport
//! and peer behavior and must not execute blocks, own consensus rules, or
//! mutate storage directly.
//!
//! ## Contents
//!
//! - `command`: Command records sent into the service loop.
//! - `event`: Event records emitted by the service loop.
//! - `handle`: Typed handle used to interact with the service task.
//! - `local_node`: Local peer state and connection behavior.
//! - `remote_node`: Remote peer state and connection behavior.

pub(crate) mod command;
pub(crate) mod event;
pub(crate) mod handle;
pub(crate) mod local_node;
pub(crate) mod remote_node;

pub use command::NetworkCommand;
pub use event::NetworkEvent;
pub use handle::{
    ConnectedPeer, DEFAULT_COMMAND_CAPACITY, DEFAULT_EVENT_CAPACITY, LocalNodeInfo, NetworkHandle,
    SharedNetworkHandle,
};
pub use local_node::LocalNodeService;
pub use remote_node::{
    BlockSource, InboundInventory, InventoryItem, RemoteNodeCommand, RemoteNodeHandle,
    RemoteNodeService, RemoteNodeState,
};
