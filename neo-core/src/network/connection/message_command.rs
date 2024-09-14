/// Represents the command of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageCommand {
    // Handshaking
    /// Sent when a connection is established.
    #[reflection_cache(VersionPayload)]
    Version = 0x00,

    /// Sent to respond to Version messages.
    Verack = 0x01,

    // Connectivity
    /// Sent to request for remote nodes.
    GetAddr = 0x10,

    /// Sent to respond to GetAddr messages.
    #[reflection_cache(AddrPayload)]
    Addr = 0x11,

    /// Sent to detect whether the connection has been disconnected.
    #[reflection_cache(PingPayload)]
    Ping = 0x18,

    /// Sent to respond to Ping messages.
    #[reflection_cache(PingPayload)]
    Pong = 0x19,

    // Synchronization
    /// Sent to request for headers.
    #[reflection_cache(GetBlockByIndexPayload)]
    GetHeaders = 0x20,

    /// Sent to respond to GetHeaders messages.
    #[reflection_cache(HeadersPayload)]
    Headers = 0x21,

    /// Sent to request for blocks.
    #[reflection_cache(GetBlocksPayload)]
    GetBlocks = 0x24,

    /// Sent to request for memory pool.
    Mempool = 0x25,

    /// Sent to relay inventories.
    #[reflection_cache(InvPayload)]
    Inv = 0x27,

    /// Sent to request for inventories.
    #[reflection_cache(InvPayload)]
    GetData = 0x28,

    /// Sent to request for blocks.
    #[reflection_cache(GetBlockByIndexPayload)]
    GetBlockByIndex = 0x29,

    /// Sent to respond to GetData messages when the inventories are not found.
    #[reflection_cache(InvPayload)]
    NotFound = 0x2a,

    /// Sent to send a transaction.
    #[reflection_cache(Transaction)]
    Transaction = 0x2b,

    /// Sent to send a block.
    #[reflection_cache(Block)]
    Block = 0x2c,

    /// Sent to send an ExtensiblePayload.
    #[reflection_cache(ExtensiblePayload)]
    Extensible = 0x2e,

    /// Sent to reject an inventory.
    Reject = 0x2f,

    // SPV protocol
    /// Sent to load the BloomFilter.
    #[reflection_cache(FilterLoadPayload)]
    FilterLoad = 0x30,

    /// Sent to update the items for the BloomFilter.
    #[reflection_cache(FilterAddPayload)]
    FilterAdd = 0x31,

    /// Sent to clear the BloomFilter.
    FilterClear = 0x32,

    /// Sent to send a filtered block.
    #[reflection_cache(MerkleBlockPayload)]
    MerkleBlock = 0x38,

    // Others
    /// Sent to send an alert.
    Alert = 0x40,
}
