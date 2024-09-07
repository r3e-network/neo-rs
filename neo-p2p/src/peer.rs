// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::time::Duration;

use trust_dns_resolver::Resolver;

use neo_base::time::{AtomicUnixTime, UnixTime};
use neo_core::payload::Version;
use crate::SeedState;


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
}


#[derive(Debug, Copy, Clone)]
pub struct Timeouts {
    pub ping: bool,
    pub handshake: bool,
}


#[derive(Debug)]
pub struct Connected {
    pub addr: SocketAddr,
    pub connected_at: UnixTime,
    pub stages: AtomicU32,
    pub ping_sent: AtomicUnixTime,
    pub ping_recv: AtomicUnixTime,
    pub pong_recv: AtomicUnixTime,
}

impl Connected {
    #[inline]
    pub fn new(addr: SocketAddr, stages: u32) -> Connected {
        Connected {
            addr,
            connected_at: UnixTime::now(),
            stages: AtomicU32::new(stages),
            ping_sent: AtomicUnixTime::default(),
            ping_recv: AtomicUnixTime::default(),
            pong_recv: AtomicUnixTime::default(),
        }
    }

    #[inline]
    pub fn stages(&self) -> u32 { self.stages.load(Relaxed) }

    #[inline]
    pub fn add_stages(&self, stages: u32) {
        let mut old = self.stages.load(Relaxed);
        while self.stages.compare_exchange(old, old | stages, Relaxed, Relaxed).is_err() {
            old = self.stages.load(Relaxed);
        }
    }

    #[inline]
    pub fn is_accepted(&self) -> bool {
        self.stages.load(Relaxed) & PeerStage::Accepted.as_u32() != 0
    }

    pub fn ping_timeout(&self, now: UnixTime, timeout: Duration) -> bool {
        let sent = self.ping_sent.load().unix_millis();
        if sent == 0 { // not sent
            return false;
        }

        let timeout = timeout.as_millis() as i64;
        let now = now.unix_millis();
        let recv = self.pong_recv.load().unix_millis();
        if recv > 0 && now - recv < timeout {
            return false;
        }

        (recv < sent && now - sent >= timeout) || (recv > sent && recv - sent >= timeout)
    }

    pub fn handshake_timeout(&self, now: UnixTime, timeout: Duration) -> bool {
        if self.stages() & PeerStage::VersionAckReceived.as_u32() != 0 {
            return false;
        }

        now.unix_millis() - self.connected_at.unix_millis() >= timeout.as_millis() as i64
    }
}


#[derive(Debug)]
pub struct TcpPeer {
    // `addr` is the connection socket address
    pub addr: SocketAddr,
    pub handshake_at: UnixTime,
    // pub ping_sent: AtomicUnixTime,
    // pub ping_recv: AtomicUnixTime,
    // pub pong_recv: AtomicUnixTime,
    // pub last_block_index: AtomicU32,
    pub score: u64,
    pub version: Version,
}

impl TcpPeer {
    #[inline]
    pub fn new(addr: SocketAddr, version: Version) -> Self {
        // let last_block_index = version.start_height().unwrap_or(0);
        Self {
            addr,
            handshake_at: UnixTime::now(),
            // ping_sent: AtomicUnixTime::default(),
            // ping_recv: AtomicUnixTime::default(),
            // pong_recv: AtomicUnixTime::default(),
            // last_block_index: AtomicU32::new(last_block_index),
            score: 0,
            version,
        }
    }

    #[inline]
    pub fn service_addr(&self) -> Option<SocketAddr> {
        self.version.port()
            .map(|x| SocketAddr::new(self.addr.ip(), x))
    }
}


#[derive(Debug, Clone)]
pub struct Seed {
    pub addr: SocketAddr,
    pub state: SeedState,
}

impl Seed {
    #[inline]
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr, state: SeedState::Reachable } // initial SeedState is `Reachable`
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

    pub(crate) fn on_start(&self) -> Vec<(String, Seed)> {
        let seeds = self.resolves();
        if seeds.len() != self.seeds.len() {
            panic!("`DnsResolver::on_start`: resolved {} != seeds {}", seeds.len(), self.seeds.len());
        }
        seeds
    }

    // TODO: resolve seeds periodically
    pub fn resolves(&self) -> Vec<(String, Seed)> {
        self.seeds.iter()
            .filter_map(|(host, port)| {
                self.resolve(host)
                    .map(|x| (format!("{}:{}", host, port), Seed::new(SocketAddr::new(x, *port))))
            })
            .collect()
    }

    fn resolve(&self, host: &str) -> Option<IpAddr> {
        match self.resolver.lookup_ip(host) {
            Ok(lookup) => {
                let addr = lookup.iter().next();
                if addr.is_none() {
                    log::error!("`loop_ip` for {} no IpAddr got", host)
                }
                addr
            }
            Err(err) => {
                log::error!("`lookup_ip` for {} err: {}", host, err);
                None
            }
        }
    }
}


#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_connected() {
        let addr = "127.0.0.1:10234".parse().unwrap();
        let connected = Connected::new(addr, PeerStage::Accepted.as_u32());
        assert_eq!(connected.is_accepted(), true);

        connected.add_stages(PeerStage::VersionSent.as_u32());
        assert_eq!(connected.stages(), PeerStage::VersionSent.as_u32() | PeerStage::Accepted.as_u32());
        assert_eq!(connected.is_accepted(), true);

        let now = UnixTime::now();
        let timeout = Duration::from_secs(2);
        let ping = connected.ping_timeout(now, timeout);
        assert_eq!(ping, false);

        let handshake = connected.handshake_timeout(now, timeout);
        assert_eq!(handshake, false);

        connected.ping_sent.store(now - Duration::from_secs(3));
        let ping = connected.ping_timeout(now, timeout);
        assert_eq!(ping, true);

        connected.ping_sent.store(now - Duration::from_secs(10));
        connected.pong_recv.store(now - Duration::from_secs(2));
        let ping = connected.ping_timeout(now, timeout);
        assert_eq!(ping, true);

        connected.ping_sent.store(now - Duration::from_secs(4));
        connected.pong_recv.store(now - Duration::from_secs(3));
        let ping = connected.ping_timeout(now, timeout);
        assert_eq!(ping, false);

        let handshake = connected.handshake_timeout(now, Duration::from_secs(0));
        assert_eq!(handshake, true);

        connected.add_stages(PeerStage::VersionAckReceived.as_u32());
        let handshake = connected.handshake_timeout(now, Duration::from_secs(0));
        assert_eq!(handshake, false);
    }

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