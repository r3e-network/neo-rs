use std::error::Error;
use std::fmt;

use crate::io::{BinReader, BinWriter, Serializable};

// MaxCapabilities is the maximum number of capabilities per payload.
const MAX_CAPABILITIES: usize = 32;

// Capabilities is a list of Capability.
#[derive(Debug, Clone)]
struct Capabilities(Vec<Capability>);

impl Serializable for Capabilities {
    fn decode(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.0 = br.read_array(MAX_CAPABILITIES)?;
        self.check_unique_capabilities()?;
        Ok(())
    }

    fn encode(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        bw.write_array(&self.0)?;
        Ok(())
    }
}

impl Capabilities {
    fn check_unique_capabilities(&self) -> Result<(), Box<dyn Error>> {
        let err = "capabilities with the same type are not allowed".to_string();
        let mut is_full_node = false;
        let mut is_tcp = false;
        let mut is_ws = false;
        for cap in &self.0 {
            match cap.cap_type {
                CapabilityType::FullNode => {
                    if is_full_node {
                        return Err(Box::new(fmt::Error::new(err.clone())));
                    }
                    is_full_node = true;
                }
                CapabilityType::TCPServer => {
                    if is_tcp {
                        return Err(Box::new(fmt::Error::new(err.clone())));
                    }
                    is_tcp = true;
                }
                CapabilityType::WSServer => {
                    if is_ws {
                        return Err(Box::new(fmt::Error::new(err.clone())));
                    }
                    is_ws = true;
                }
            }
        }
        Ok(())
    }
}

// Capability describes a network service available for the node.
#[derive(Debug, Clone)]
struct Capability {
    cap_type: CapabilityType,
    data: Box<dyn Serializable>,
}

impl Serializable for Capability {
    fn decode(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.cap_type = CapabilityType::from(br.read_u8()?);
        self.data = match self.cap_type {
            CapabilityType::FullNode => Box::new(Node::default()),
            CapabilityType::TCPServer | CapabilityType::WSServer => Box::new(Server::default()),
            _ => return Err(Box::new(fmt::Error::new("unknown node capability type".to_string()))),
        };
        self.data.decode(br)?;
        Ok(())
    }

    fn encode(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        if self.data.is_none() {
            return Err(Box::new(fmt::Error::new("capability has no data".to_string())));
        }
        bw.write_u8(self.cap_type as u8)?;
        self.data.encode(bw)?;
        Ok(())
    }
}

// Node represents full node capability with a start height.
#[derive(Debug, Clone, Default)]
struct Node {
    start_height: u32,
}

impl Serializable for Node {
    fn decode(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.start_height = br.read_u32_le()?;
        Ok(())
    }

    fn encode(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        bw.write_u32_le(self.start_height)?;
        Ok(())
    }
}

// Server represents TCP or WS server capability with a port.
#[derive(Debug, Clone, Default)]
struct Server {
    port: u16,
}

impl Serializable for Server {
    fn decode(&mut self, br: &mut BinReader) -> Result<(), Box<dyn Error>> {
        self.port = br.read_u16_le()?;
        Ok(())
    }

    fn encode(&self, bw: &mut BinWriter) -> Result<(), Box<dyn Error>> {
        bw.write_u16_le(self.port)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum CapabilityType {
    FullNode,
    TCPServer,
    WSServer,
}

impl From<u8> for CapabilityType {
    fn from(value: u8) -> Self {
        match value {
            0 => CapabilityType::FullNode,
            1 => CapabilityType::TCPServer,
            2 => CapabilityType::WSServer,
            _ => panic!("unknown capability type"),
        }
    }
}
