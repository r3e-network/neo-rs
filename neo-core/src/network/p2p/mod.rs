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

//! P2P networking module matching C# `Neo.Network.P2P`.
//!
//! # Security Warning (H-6)
//!
//! **IMPORTANT**: P2P communications in this module are **NOT ENCRYPTED**.
//!
//! All network traffic between Neo nodes is transmitted in plaintext, which means:
//!
//! - **Eavesdropping**: Network observers can see all P2P messages including transactions,
//!   blocks, and consensus messages.
//! - **Man-in-the-Middle**: Attackers on the network path could potentially intercept and
//!   modify messages (though consensus signatures provide some protection).
//! - **Traffic Analysis**: Network patterns can reveal node behavior and relationships.
//!
//! ## Mitigations
//!
//! For production deployments, consider:
//!
//! 1. **VPN/Tunnel**: Run P2P traffic over an encrypted tunnel (WireGuard, IPsec)
//! 2. **Private Network**: Deploy nodes on isolated private networks
//! 3. **Tor/I2P**: Use anonymizing networks for additional privacy
//! 4. **Firewall Rules**: Restrict P2P connections to known trusted peers
//!
//! ## Why No Built-in Encryption?
//!
//! This matches the C# Neo reference implementation which also uses unencrypted TCP.
//! The Neo protocol relies on cryptographic signatures for message authenticity rather
//! than transport-layer encryption. Adding TLS would break compatibility with the
//! existing Neo network.
//!
//! ## Future Considerations
//!
//! A future protocol upgrade could add optional encryption (e.g., Noise Protocol Framework)
//! while maintaining backward compatibility through capability negotiation.

pub mod capabilities;
pub mod channels_config;
pub mod connection;
pub mod framed;
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
pub mod timeouts;

// Re-export commonly used types
pub use channels_config::ChannelsConfig;
pub use connection::PeerConnection;
pub use framed::FrameConfig;
pub use helper::{get_sign_data, get_sign_data_vec};
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
