//! Shared P2P message payload limit.

/// Maximum payload size (matches `Neo.Network.P2P.Message.PayloadMaxSize`)
pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000; // 32 MiB
