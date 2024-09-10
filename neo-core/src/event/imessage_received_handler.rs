use crate::neo_system::NeoSystem;
use crate::network::Message;

/// Trait for handling messages received from remote nodes in the Neo network.
pub trait IMessageReceivedHandler {
    /// Handler for the MessageReceived event from a RemoteNode.
    ///
    /// This method is triggered when a new message is received from a peer RemoteNode.
    ///
    /// # Arguments
    ///
    /// * `system` - A reference to the NeoSystem object.
    /// * `message` - The Message received from a peer RemoteNode.
    ///
    /// # Returns
    ///
    /// Returns a boolean indicating whether the message was handled successfully.
    fn remote_node_message_received_handler(&self, system: &NeoSystem, message: &Message) -> bool;
}
