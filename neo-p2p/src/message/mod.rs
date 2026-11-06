//! Neo `Message` implementation (mirrors the C# class) with optimized serialization.

/// Maximum payload size (matches `Neo.Network.P2P.Message.PayloadMaxSize`)
pub const PAYLOAD_MAX_SIZE: usize = 0x0200_0000; // 32 MiB

// Full Message implementation moved from neo-core (task D5-2)
mod full;
pub use full::Message;
