#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CapabilityType {
    TcpServer = 0x01,
    WsServer = 0x02,
    DisableCompression = 0x03,
    FullNode = 0x10,
    ArchivalNode = 0x11,
    Extension0 = 0xF0,
    Unknown(u8),
}

impl CapabilityType {
    pub fn from_byte(value: u8) -> Self {
        match value {
            0x01 => CapabilityType::TcpServer,
            0x02 => CapabilityType::WsServer,
            0x03 => CapabilityType::DisableCompression,
            0x10 => CapabilityType::FullNode,
            0x11 => CapabilityType::ArchivalNode,
            0xF0 => CapabilityType::Extension0,
            other => CapabilityType::Unknown(other),
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            CapabilityType::TcpServer => 0x01,
            CapabilityType::WsServer => 0x02,
            CapabilityType::DisableCompression => 0x03,
            CapabilityType::FullNode => 0x10,
            CapabilityType::ArchivalNode => 0x11,
            CapabilityType::Extension0 => 0xF0,
            CapabilityType::Unknown(value) => value,
        }
    }
}
