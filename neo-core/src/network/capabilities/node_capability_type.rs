
/// Represents the type of NodeCapability.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeCapabilityType {
    // Servers
    /// Indicates that the node is listening on a TCP port.
    TcpServer = 0x01,

    /// Indicates that the node is listening on a WebSocket port.
    #[deprecated]
    WsServer = 0x02,

    // Others
    /// Indicates that the node has complete block data.
    FullNode = 0x10,
}

impl From<u8> for NodeCapabilityType {
    fn from(value: u8) -> Self {
        match value {
            0x01 => NodeCapabilityType::TcpServer,
            0x02 => NodeCapabilityType::WsServer,
            0x10 => NodeCapabilityType::FullNode,
            _ => panic!("Invalid NodeCapabilityType value"),
        }
    }
}
