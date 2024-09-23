use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use std::time::{SystemTime, UNIX_EPOCH};
use std::convert::TryInto;
use std::error::Error;
use std::fmt;

use crate::io::{BinReader, BinWriter, Serializable};
use crate::network::capability::{Capabilities, Capability, CapabilityType, Server};

// MaxAddrsCount is the maximum number of addresses that could be packed into
// one payload.
pub const MAX_ADDRS_COUNT: usize = 200;

// AddressAndTime payload.
#[derive(Debug, Clone)]
pub struct AddressAndTime {
    pub timestamp: u32,
    pub ip: [u8; 16],
    pub capabilities: Capabilities,
}

impl AddressAndTime {
    // NewAddressAndTime creates a new AddressAndTime object.
    pub fn new(e: &SocketAddr, t: SystemTime, c: Capabilities) -> Self {
        let duration = t.duration_since(UNIX_EPOCH).expect("Time went backwards");
        let mut ip = [0u8; 16];
        if let IpAddr::V6(ipv6) = e.ip() {
            ip.copy_from_slice(&ipv6.octets());
        }
        AddressAndTime {
            timestamp: duration.as_secs() as u32,
            ip,
            capabilities: c,
        }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        self.timestamp = br.read_u32_le();
        br.read_bytes(&mut self.ip);
        self.capabilities.decode_binary(br);
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_u32_le(self.timestamp);
        bw.write_bytes(&self.ip);
        self.capabilities.encode_binary(bw);
    }

    // GetTCPAddress makes a string from the IP and the port specified in TCPCapability.
    // It returns an error if there's no such capability.
    pub fn get_tcp_address(&self) -> Result<String, Box<dyn Error>> {
        let netip = Ipv6Addr::from(self.ip);
        let mut port = None;
        for cap in &self.capabilities {
            if cap.cap_type == CapabilityType::TCPServer {
                if let Some(server) = cap.data.downcast_ref::<Server>() {
                    port = Some(server.port);
                    break;
                }
            }
        }
        match port {
            Some(port) => Ok(format!("[{}]:{}", netip, port)),
            None => Err(Box::new(TCPError::NoCapability)),
        }
    }
}

#[derive(Debug)]
enum TCPError {
    NoCapability,
}

impl fmt::Display for TCPError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no TCP capability found")
    }
}

impl Error for TCPError {}

// AddressList is a list with AddrAndTime.
#[derive(Debug, Clone)]
pub struct AddressList {
    pub addrs: Vec<AddressAndTime>,
}

impl AddressList {
    // NewAddressList creates a list for n AddressAndTime elements.
    pub fn new(n: usize) -> Self {
        AddressList {
            addrs: Vec::with_capacity(n),
        }
    }

    // DecodeBinary implements the Serializable interface.
    pub fn decode_binary(&mut self, br: &mut BinReader) {
        br.read_array(&mut self.addrs, MAX_ADDRS_COUNT);
        if self.addrs.is_empty() {
            br.set_err(Box::new(TCPError::NoCapability));
        }
    }

    // EncodeBinary implements the Serializable interface.
    pub fn encode_binary(&self, bw: &mut BinWriter) {
        bw.write_array(&self.addrs);
    }
}
