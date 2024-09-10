// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::{SeedState::*, *};
use neo_base::math::LcgRand;
use neo_base::time::UnixTime;

const CONNECT_RETRY_TIMES: u32 = 3;
const MAX_POOL_SIZE: usize = 1024;

#[derive(Debug, Clone)]
pub struct Discoveries {
    pub net_size: u32,
    pub fan_out: u32,
    pub unconnected: u32,
    pub connected: u32,
    pub goods: u32,
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum SeedState {
    Temporary,
    Permanent,
    Reachable,
}

#[allow(dead_code)]
#[derive(Copy, Clone)]
pub(crate) struct Knew {
    pub when: UnixTime,
    pub remain_times: u32,
}

impl Knew {
    #[inline]
    pub fn new() -> Self {
        Knew {
            when: UnixTime::now(),
            remain_times: CONNECT_RETRY_TIMES,
        }
    }
}

pub type Discovery = Arc<Mutex<DiscoveryV1<mpsc::Sender<SocketAddr>>>>;

// stages: seeds -> unconnected -> attempts -> connected -> goods or failures
#[allow(dead_code)]
pub struct DiscoveryV1<Dial: crate::Dial> {
    dial: Dial,
    resolver: DnsResolver,
    seeds: Vec<(String, Seed)>, // HashMap<String, Seed>,

    // knew but not connected
    unconnected: HashMap<SocketAddr, Knew>,
    attempts: HashMap<SocketAddr, UnixTime>,
    connected: HashMap<SocketAddr, Connected>,

    // TODO: remove from failures after some time
    failures: HashMap<SocketAddr, UnixTime>,
    goods: HashMap<SocketAddr, TcpPeer>,

    // nonce -> service address
    nodes: HashMap<u32, SocketAddr>,

    // i.e. optimal fan out
    fan_out: u32,
    net_size: u32,
}

impl<Dial: crate::Dial> DiscoveryV1<Dial> {
    pub fn new(dial: Dial, resolver: DnsResolver) -> Self {
        let seeds = resolver.on_start();
        Self {
            dial,
            resolver,
            seeds,
            unconnected: HashMap::new(),
            attempts: HashMap::new(),
            connected: HashMap::new(),
            failures: HashMap::new(),
            goods: HashMap::new(),
            nodes: HashMap::new(),
            fan_out: 1,
            net_size: 0,
        }
    }

    #[inline]
    pub fn with_shared(dial: Dial, resolver: DnsResolver) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new(dial, resolver)))
    }

    fn update_net_size(&mut self) {
        let net_size = self.connected.len() + self.unconnected.len() + 1; // +1 is itself
        let fan_out = if net_size > 2 {
            2.5 * ((net_size - 1) as f64).ln()
        } else {
            1.0
        };

        self.fan_out = (fan_out + 0.5) as u32;
        self.net_size = net_size as u32;
    }

    fn seed_mut(&mut self, addr: &SocketAddr) -> Option<&mut Seed> {
        self.seeds
            .iter_mut()
            .find(|(_, x)| x.addr.eq(&addr))
            .map(|(_, x)| x)
    }

    #[inline]
    fn add_unconnected(&mut self, addr: SocketAddr) -> bool {
        let not_full = self.unconnected.len() < MAX_POOL_SIZE;
        if not_full {
            self.unconnected.insert(addr, Knew::new());
            log::info!("peer {} added to unconnected", &addr);
        } else {
            log::warn!("peer {} not add to unconnected on fulled", &addr);
        }
        not_full
    }

    fn add_failure(&mut self, addr: SocketAddr) {
        self.unconnected.remove(&addr);
        self.connected.remove(&addr);
        self.attempts.remove(&addr);
        if let Some(peer) = self.goods.remove(&addr) {
            self.nodes.remove(&peer.version.nonce);
        }

        self.failures.insert(addr, UnixTime::now());
        self.update_net_size();
    }

    #[inline]
    fn try_connect(&mut self, addr: SocketAddr) {
        self.attempts.insert(addr, UnixTime::now());
        if let Err(_err) = self.dial.dial(addr) {
            self.attempts.remove(&addr);
        }
    }

    fn request_seed(&self) -> Option<SocketAddr> {
        let mut rand = LcgRand::new(UnixTime::now().unix_micros() as u64);
        let mut select = |state: SeedState| {
            let mut addr = None;
            for (idx, (_, seed)) in self.seeds.iter().enumerate() {
                if seed.state == state
                    && !self.attempts.contains_key(&seed.addr)
                    && (addr.is_none() || rand.next() % (idx as u64 + 1) == 0)
                {
                    addr = Some(&seed.addr);
                }
            }
            addr
        };

        select(Reachable).or_else(|| select(Temporary)).cloned()
    }
}

impl<Dial: crate::Dial> DiscoveryV1<Dial> {
    // addrs should be service address list
    pub fn back_fill(&mut self, addrs: &[SocketAddr]) {
        for addr in addrs {
            if self.failures.contains_key(addr)
                || self.connected.contains_key(addr)
                || self
                    .unconnected
                    .get(addr)
                    .is_some_and(|d| d.remain_times > 0)
            {
                continue;
            }
            self.add_unconnected(addr.clone());
        }

        self.update_net_size();
    }

    pub fn request_remotes(&mut self, nr_remotes: u32) {
        let outstanding = self.attempts.len() as u32;
        if outstanding >= nr_remotes {
            return;
        }

        for _ in 0..(nr_remotes - outstanding) {
            let next = self
                .unconnected
                .iter()
                .find(|(k, _)| !self.connected.contains_key(k) && !self.attempts.contains_key(k))
                .map(|(k, _)| k.clone())
                .or_else(|| self.request_seed());
            if let Some(next) = next {
                // log::info!("request_remotes next {}", &next);
                self.try_connect(next);
            }
        }
    }

    // `addr` is client or server socket addr
    pub fn on_incoming(&mut self, addr: SocketAddr, stage: u32) {
        self.unconnected.remove(&addr);
        self.attempts.remove(&addr); // removed only if `peer` is server socket addr
        self.connected
            .entry(addr)
            .and_modify(|x| x.add_stages(stage))
            .or_insert(Connected::new(addr, stage));
        self.update_net_size();
    }

    // `service` is the peer tcp-server socket addr
    pub fn on_good(&mut self, peer: TcpPeer) {
        let Some(service) = peer.service_addr() else {
            return;
        };
        self.unconnected.remove(&service);
        self.failures.remove(&service);
        self.attempts.remove(&service);

        self.nodes.insert(peer.version.nonce, service);
        // self.goods.insert(service, peer);
        self.goods.insert(peer.addr, peer);

        // self.seed_mut(&service)
        //     .filter(|seed| seed.state != Permanent)
        //     .map(|seed| seed.state = Reachable);
    }

    // `addr` may be client or server socket addr
    pub fn on_disconnected(&mut self, addr: &SocketAddr) {
        self.connected.remove(addr);
        self.attempts.remove(addr);
        self.seed_mut(addr)
            .filter(|seed| seed.state != Permanent)
            .map(|seed| seed.state = Temporary);

        let Some(peer) = self.goods.remove(addr) else {
            return;
        };
        self.nodes.remove(&peer.version.nonce);

        let Some(service) = peer.service_addr() else {
            return;
        };
        self.seed_mut(&service) // if peer is client socket addr
            .filter(|seed| seed.state != Permanent)
            .map(|seed| seed.state = Temporary);

        self.back_fill(core::slice::from_ref(&service)); // back fill server socket addr
    }

    // `addr` is the local listen addr
    pub fn on_failure_always(&mut self, service: SocketAddr) {
        if let Some(seed) = self.seed_mut(&service) {
            seed.state = Permanent;
            self.attempts.remove(&service);
            self.connected.remove(&service);
            self.unconnected.remove(&service);
        } else {
            self.add_failure(service)
        }
    }

    // `service` is the server socket addr. `on_failure` is called when `connect` failed
    pub fn on_failure(&mut self, service: &SocketAddr) {
        if let Some(seed) = self.seed_mut(service) {
            if seed.state != Permanent {
                seed.state = Temporary;
            }
            self.attempts.remove(&service);
            self.connected.remove(&service);
            self.unconnected.remove(&service);
            // log::info!("`on_failure` for {}", &service);
        } else {
            self.unconnected
                .get_mut(&service)
                .filter(|v| v.remain_times > 0)
                .map(|v| {
                    v.remain_times -= 1;
                    v.remain_times as i32 <= 0
                })
                .unwrap_or(true)
                .then(|| {
                    self.add_failure(service.clone());
                });
        }
    }

    // `addr` may be client or server socket addr
    #[inline]
    pub fn connected(&self, addr: &SocketAddr) -> Option<&Connected> {
        self.connected.get(addr)
    }

    #[inline]
    pub fn good(&self, addr: &SocketAddr) -> Option<&TcpPeer> {
        self.goods.get(addr)
    }

    #[inline]
    pub fn has_peer(&self, service: &SocketAddr, nonce: u32) -> bool {
        self.goods.contains_key(service) || self.nodes.contains_key(&nonce)
    }

    #[inline]
    pub fn connected_peers(&self) -> impl Iterator<Item = &Connected> {
        self.connected.iter().map(|(_, peer)| peer)
    }

    #[inline]
    pub fn good_peers(&self) -> impl Iterator<Item = &TcpPeer> {
        self.goods.iter().map(|(_, peer)| peer)
    }

    #[inline]
    pub fn discoveries(&self) -> Discoveries {
        Discoveries {
            net_size: self.net_size,
            fan_out: self.fan_out,
            unconnected: self.unconnected.len() as u32,
            connected: self.connected.len() as u32,
            goods: self.goods.len() as u32,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use neo_base::time::unix_seconds_now;
    use neo_core::payload::{Capability, Version};

    #[test]
    fn test_discovery_v1() {
        let seed = "seed1t5.neo.org:20333";
        let (tx, mut rx) = mpsc::channel(128);
        let dns = DnsResolver::new(&[seed.into()]);

        let mut disc = DiscoveryV1::new(tx, dns);
        disc.request_remotes(2);

        let addr = rx.try_recv().expect("`try_recv` should be ok");

        let _ = rx.try_recv().expect_err("`try_recv` should be failed");

        assert!(disc.attempts.contains_key(&addr));

        disc.on_incoming(addr, PeerStage::Connected.as_u32());
        let Some(connected) = disc.connected(&addr) else {
            panic!("should be exists");
        };
        assert!(PeerStage::Connected.belongs(connected.stages()));
        assert!(!disc.attempts.contains_key(&addr));

        disc.on_good(TcpPeer::new(
            addr,
            Version {
                network: Network::DevNet.as_magic(),
                version: 0,
                unix_seconds: unix_seconds_now() as u32,
                nonce: 12345,
                user_agent: "x".into(),
                capabilities: vec![Capability::TcpServer { port: addr.port() }],
            },
        ));

        let Some(peer) = disc.good(&addr) else {
            panic!("should be exists");
        };
        assert_eq!(peer.addr, addr);

        disc.on_disconnected(&addr);
        assert!(!disc.attempts.contains_key(&addr));
        assert!(!disc.connected.contains_key(&addr));
        assert!(disc.unconnected.contains_key(&addr));
        assert!(!disc.failures.contains_key(&addr));

        disc.on_failure_always(addr);
        assert!(!disc.attempts.contains_key(&addr));
        assert!(!disc.connected.contains_key(&addr));
        assert!(!disc.unconnected.contains_key(&addr));
        assert!(!disc.failures.contains_key(&addr)); // seed

        let (service, state) = &disc.seeds[0];
        assert_eq!(service.as_str(), seed);
        assert_eq!(state.state, Permanent);
    }
}
