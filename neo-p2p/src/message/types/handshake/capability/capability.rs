use neo_base::encoding::{
    read_varint, write_varint, DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite,
};

use super::CapabilityType;

pub const MAX_UNKNOWN_CAPABILITY_DATA: u64 = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    TcpServer { port: u16 },
    WsServer { port: u16 },
    DisableCompression,
    FullNode { start_height: u32 },
    ArchivalNode,
    Unknown { ty: CapabilityType, data: Vec<u8> },
}

impl Capability {
    pub fn tcp_server(port: u16) -> Self {
        Self::TcpServer { port }
    }

    pub fn ws_server(port: u16) -> Self {
        Self::WsServer { port }
    }

    pub fn full_node(start_height: u32) -> Self {
        Self::FullNode { start_height }
    }

    pub fn disable_compression() -> Self {
        Self::DisableCompression
    }

    pub fn archival_node() -> Self {
        Self::ArchivalNode
    }

    pub fn capability_type(&self) -> CapabilityType {
        match self {
            Capability::TcpServer { .. } => CapabilityType::TcpServer,
            Capability::WsServer { .. } => CapabilityType::WsServer,
            Capability::DisableCompression => CapabilityType::DisableCompression,
            Capability::FullNode { .. } => CapabilityType::FullNode,
            Capability::ArchivalNode => CapabilityType::ArchivalNode,
            Capability::Unknown { ty, .. } => *ty,
        }
    }
}

impl NeoEncode for Capability {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        match self {
            Capability::TcpServer { port } => {
                writer.write_u8(CapabilityType::TcpServer.to_byte());
                writer.write_u16(*port);
            }
            Capability::WsServer { port } => {
                writer.write_u8(CapabilityType::WsServer.to_byte());
                writer.write_u16(*port);
            }
            Capability::DisableCompression => {
                writer.write_u8(CapabilityType::DisableCompression.to_byte());
                writer.write_u8(0);
            }
            Capability::FullNode { start_height } => {
                writer.write_u8(CapabilityType::FullNode.to_byte());
                writer.write_u32(*start_height);
            }
            Capability::ArchivalNode => {
                writer.write_u8(CapabilityType::ArchivalNode.to_byte());
                writer.write_u8(0);
            }
            Capability::Unknown { ty, data } => {
                writer.write_u8(ty.to_byte());
                write_varint(writer, data.len() as u64);
                writer.write_bytes(data);
            }
        }
    }
}

impl NeoDecode for Capability {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let ty = CapabilityType::from_byte(reader.read_u8()?);
        let capability = match ty {
            CapabilityType::TcpServer => {
                let port = reader.read_u16()?;
                Capability::TcpServer { port }
            }
            CapabilityType::WsServer => {
                let port = reader.read_u16()?;
                Capability::WsServer { port }
            }
            CapabilityType::DisableCompression => {
                let zero = reader.read_u8()?;
                if zero != 0 {
                    return Err(DecodeError::InvalidValue("DisableCompression payload"));
                }
                Capability::DisableCompression
            }
            CapabilityType::FullNode => {
                let start_height = reader.read_u32()?;
                Capability::FullNode { start_height }
            }
            CapabilityType::ArchivalNode => {
                let zero = reader.read_u8()?;
                if zero != 0 {
                    return Err(DecodeError::InvalidValue("ArchivalNode payload"));
                }
                Capability::ArchivalNode
            }
            CapabilityType::Extension0 | CapabilityType::Unknown(_) => {
                let len = read_varint(reader)?;
                if len > MAX_UNKNOWN_CAPABILITY_DATA {
                    return Err(DecodeError::LengthOutOfRange {
                        len,
                        max: MAX_UNKNOWN_CAPABILITY_DATA,
                    });
                }
                let mut data = vec![0u8; len as usize];
                reader.read_into(&mut data)?;
                Capability::Unknown { ty, data }
            }
        };

        Ok(capability)
    }
}
