//! Message flag definitions (mirrors `Neo.Network.P2P.MessageFlags`).

use neo_primitives::protocol_message_flags;

protocol_message_flags! {
    /// Message flags applied to the network payload header.
    ///
    /// The C# implementation treats this as a `[Flags]` enum, so we preserve the raw
    /// byte rather than rejecting unknown combinations. Future protocol extensions
    /// can therefore add bits without breaking the Rust node.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub MessageFlags {
        warn_target = "neo::p2p";
        from_byte = from_byte;
    }
}

#[cfg(test)]
#[path = "../tests/proto/message_flags.rs"]
mod tests;
