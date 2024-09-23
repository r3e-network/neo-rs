use serde::{Deserialize, Serialize};
use std::net::{ToSocketAddrs, SocketAddr};
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug)]
pub struct GetPeers {
    unconnected: Peers,
    connected: Peers,
    bad: Peers,
}

impl GetPeers {
    pub fn new() -> Self {
        GetPeers {
            unconnected: vec![],
            connected: vec![],
            bad: vec![],
        }
    }

    pub fn add_unconnected(&mut self, addrs: Vec<String>) {
        self.unconnected.add_peers(addrs);
    }

    pub fn add_connected(&mut self, connected_peers: Vec<network::PeerInfo>) {
        self.connected.add_connected_peers(connected_peers);
    }

    pub fn add_bad(&mut self, addrs: Vec<String>) {
        self.bad.add_peers(addrs);
    }
}

pub type Peers = Vec<Peer>;

#[derive(Serialize, Deserialize, Debug)]
pub struct Peer {
    address: String,
    port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    useragent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lastknownheight: Option<u32>,
}

impl Peers {
    fn add_peers(&mut self, addrs: Vec<String>) {
        for addr in addrs {
            if let Ok((host, port)) = parse_host_port(&addr) {
                let peer = Peer {
                    address: host,
                    port,
                    useragent: None,
                    lastknownheight: None,
                };
                self.push(peer);
            }
        }
    }

    fn add_connected_peers(&mut self, connected_peers: Vec<network::PeerInfo>) {
        for peer_info in connected_peers {
            if let Ok((host, port)) = parse_host_port(&peer_info.address) {
                let peer = Peer {
                    address: host,
                    port,
                    useragent: Some(peer_info.useragent),
                    lastknownheight: Some(peer_info.height),
                };
                self.push(peer);
            }
        }
    }
}

impl<'de> Deserialize<'de> for Peer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct NewPeer {
            address: String,
            port: u16,
            #[serde(skip_serializing_if = "Option::is_none")]
            useragent: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            lastknownheight: Option<u32>,
        }

        #[derive(Deserialize)]
        struct OldPeer {
            address: String,
            port: String,
        }

        let np = NewPeer::deserialize(deserializer);
        if let Ok(np) = np {
            return Ok(Peer {
                address: np.address,
                port: np.port,
                useragent: np.useragent,
                lastknownheight: np.lastknownheight,
            });
        }

        let op = OldPeer::deserialize(deserializer)?;
        let port = u16::from_str(&op.port).map_err(serde::de::Error::custom)?;
        Ok(Peer {
            address: op.address,
            port,
            useragent: None,
            lastknownheight: None,
        })
    }
}

fn parse_host_port(addr: &str) -> Result<(String, u16), std::io::Error> {
    let socket_addr = SocketAddr::from_str(addr)?;
    Ok((socket_addr.ip().to_string(), socket_addr.port()))
}
