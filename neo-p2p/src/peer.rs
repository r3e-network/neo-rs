// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use trust_dns_resolver::Resolver;

use neo_core::payload::Version;
use crate::{local_now, LocalTime, SeedState};


#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum PeerStage {
    Connected = 0x01,
    Accepted = 0x02,
    VersionSent = 0x04,
    VersionReceived = 0x08,
    VersionAckSent = 0x10,
    VersionAckReceived = 0x20,
}

impl PeerStage {
    #[inline]
    pub const fn as_u32(self) -> u32 { self as u32 }

    #[inline]
    pub fn belongs(self, stages: u32) -> bool {
        (self.as_u32() & stages) != 0
    }

    // pub const fn hand_shook() -> u32 {
    //     use PeerStage::*;
    //     VersionSent.as_u32() | VersionReceived.as_u32() | VersionAckSent.as_u32() | VersionAckReceived.as_u32()
    // }
}


#[derive(Debug)]
pub struct TcpPeer {
    // `addr` is the connection socket address
    pub addr: SocketAddr,
    pub handshake_at: LocalTime,
    pub score: u64,
    pub last_block_index: u32,
    pub version: Version,
}

impl TcpPeer {
    #[inline]
    pub fn new(addr: SocketAddr, version: Version) -> Self {
        let last_block_index = version.start_height().unwrap_or(0);
        Self { addr, handshake_at: local_now(), score: 0, last_block_index, version }
    }

    #[inline]
    pub fn service_addr(&self) -> Option<SocketAddr> {
        self.version.port()
            .map(|x| SocketAddr::new(self.addr.ip(), x))
    }

    #[inline]
    pub fn full_node(&self) -> bool { self.version.full_node() }
}


#[derive(Debug, Clone)]
pub struct Seed {
    pub addr: SocketAddr,
    pub state: SeedState,
}

impl Seed {
    #[inline]
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr, state: SeedState::Temporary }
    }

    #[inline]
    pub fn temporary(&self) -> bool {
        matches!(self.state , SeedState::Temporary)
    }
}


pub struct DnsResolver {
    seeds: Vec<(String, u16)>,
    resolver: Resolver,
}

impl DnsResolver {
    pub fn new(seeds: &[String]) -> Self {
        let resolver = Resolver::from_system_conf()
            .expect("`Resolver::from_system_conf()` should be ok");

        let seeds = seeds.iter()
            .map(|x| {
                let d = x.rfind(":").expect(&format!("Seed {} is invalid", x));
                let port = x[d + 1..].parse().expect(&format!("Port in seed {} is invalid", x));
                (x[..d].to_string(), port)
            })
            .collect();

        Self { seeds, resolver }
    }

    pub(crate) fn on_start(&self) -> HashMap<String, Seed> {
        let seeds = self.resolves();
        if seeds.len() != self.seeds.len() {
            panic!("`DnsResolver::on_start`: resolved {} != seeds {}", seeds.len(), self.seeds.len());
        }
        seeds
    }

    pub fn resolves(&self) -> HashMap<String, Seed> {
        self.seeds.iter()
            .filter_map(|(host, port)| {
                self.resolve(host)
                    .map(|x| (format!("{}:{}", host, port), Seed::new(SocketAddr::new(x, *port))))
            })
            .collect()
    }

    fn resolve(&self, host: &str) -> Option<IpAddr> {
        match self.resolver.lookup_ip(host) {
            Ok(lookup) => { lookup.iter().next() }
            Err(_err) => { /*TODO: log error */ None }
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_dns_resolver() {
        let dns = DnsResolver::new(&["seed1t5.neo.org:20333".into()]);
        assert_eq!(dns.seeds.len(), 1);
        assert_eq!(dns.seeds[0].0, "seed1t5.neo.org");
        assert_eq!(dns.seeds[0].1, 20333);

        let seeds = dns.on_start();
        assert_eq!(seeds.len(), 1);
    }
}