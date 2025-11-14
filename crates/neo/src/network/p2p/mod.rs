// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! P2P networking module matching C# Neo.Network.P2P

pub mod capabilities;
pub mod channels_config;
pub mod connection;
pub mod helper;
pub mod local_node;
pub mod message;
pub mod message_command;
pub mod message_flags;
pub mod messages;
pub mod payloads;
pub mod peer;
pub mod remote_node;
pub mod task_manager;
pub mod task_session;

// Re-export commonly used types
pub use channels_config::ChannelsConfig;
pub use connection::PeerConnection;
pub use helper::Helper;
pub use local_node::{
    BroadcastEvent, LocalNode, LocalNodeActor, LocalNodeCommand, RelayInventory, RemoteNodeSnapshot,
};
pub use message::Message;
pub use message_command::MessageCommand;
pub use message_flags::MessageFlags;
pub use messages::{MessageHeader, NetworkMessage, ProtocolMessage};
pub use peer::{ConnectedPeer, PeerCommand, PeerState, PeerTimer, MAX_COUNT_FROM_SEED_LIST};
pub use remote_node::{
    register_message_received_handler, unregister_message_received_handler,
    MessageHandlerSubscription, RemoteNode, RemoteNodeCommand,
};
pub use task_manager::{TaskManager, TaskManagerActor, TaskManagerCommand};
pub use task_session::TaskSession;
