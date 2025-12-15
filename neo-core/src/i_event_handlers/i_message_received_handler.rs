// Copyright (C) 2015-2025 The Neo Project.
//
// i_message_received_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::network::message::Message;
use std::any::Any;

/// Message received handler interface matching C# IMessageReceivedHandler exactly
///
/// Note: The `system` parameter uses `dyn Any` to decouple from the concrete
/// NeoSystem type which is now in neo-node. Implementations can downcast
/// to the concrete type if needed.
pub trait IMessageReceivedHandler {
    /// The handler of MessageReceived event from RemoteNode
    /// Triggered when a new message is received from a peer RemoteNode
    /// Matches C# RemoteNode_MessageReceived_Handler method
    fn remote_node_message_received_handler(&self, system: &dyn Any, message: &Message) -> bool;
}
