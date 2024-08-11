// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, atomic::{AtomicU32, Ordering}};

use tokio::sync::mpsc;

use crate::{*, SeedState::*};


const CONNECT_RETRY_TIMES: u32 = 3;
const MAX_POOL_SIZE: usize = 1024;


#[derive(Debug, Clone)]
pub struct Discoveries {
    pub net_size: u32,
    pub fan_out: u32,
    pub unconnected: u32,
}

#[derive(Debug, Clone)]
pub enum SeedState {
    Temporary,
    Permanent,
    Reachable,
}

#[allow(dead_code)]
#[derive(Copy, Clone)]
pub(crate) struct Knew {
    pub when: LocalTime,
    pub remain_times: u32,
}

impl Knew {
    #[inline]
    pub fn new() -> Self {
        Knew { when: local_now(), remain_times: CONNECT_RETRY_TIMES }
    }
}


pub struct Connected {
    pub when: LocalTime,
    pub stage: AtomicU32,
}

impl Connected {
    #[inline]
    pub fn new(stage: u32) -> Connected {
        Connected {
            when: local_now(),
            stage: AtomicU32::new(stage),
        }
    }

    pub fn add_stage(&self, stage: u32) {
        use Ordering::SeqCst;
        let mut old = self.stage.load(SeqCst);
        while self.stage.compare_exchange(old, old | stage, SeqCst, SeqCst).is_err() {
            old = self.stage.load(SeqCst);
        }
    }

    #[inline]
    pub fn is_accepted(&self) -> bool {
        self.stage.load(Ordering::SeqCst) & PeerStage::Accepted.as_u32() != 0
    }
}


pub type Discovery = DiscoveryV1<mpsc::Sender<SocketAddr>>;

pub type SharedDiscovery = Arc<Mutex<Discovery>>;


// stages: seeds -> unconnected -> attempts -> connected -> goods or failures
#[allow(dead_code)]
pub struct DiscoveryV1<Dial: crate::Dial> {
    dial: Dial,
    resolver: DnsResolver,
    seeds: HashMap<String, Seed>,

    unconnected: HashMap<SocketAddr, Knew>,
    attempts: HashMap<SocketAddr, LocalTime>,
    connected: HashMap<SocketAddr, Connected>,

    failures: HashMap<SocketAddr, LocalTime>,
    goods: HashMap<SocketAddr, TcpPeer>,

    // optimal fan out
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
            failures: HashMap::new(),
            goods: HashMap::new(),
            connected: HashMap::new(),
            unconnected: HashMap::new(),
            attempts: HashMap::new(),
            fan_out: 1,
            net_size: 0,
        }
    }

    #[inline]
    pub fn with_shared(dial: Dial, resolver: DnsResolver) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::new(dial, resolver)))
    }

    fn update_net_size(&mut self) {
        let net_size = self.connected.len() + self.unconnected.len() + 1; // +1 is self
        let fan_out = if net_size > 2 { 2.5 * ((net_size - 1) as f64).ln() } else { 1.0 };

        self.fan_out = (fan_out + 0.5) as u32;
        self.net_size = net_size as u32;

        // TODO: log net-size and disconnects.len();
    }

    fn seed_mut(&mut self, addr: &SocketAddr) -> Option<&mut Seed> {
        self.seeds.iter_mut()
            .find(|(_, x)| x.addr.eq(&addr))
            .map(|(_, x)| x)
    }

    #[inline]
    fn add_unconnected(&mut self, addr: SocketAddr) -> bool {
        let not_full = self.unconnected.len() < MAX_POOL_SIZE;
        if not_full {
            self.unconnected.insert(addr, Knew::new());
        }
        not_full
    }

    #[inline]
    fn add_failure(&mut self, addr: SocketAddr) {
        self.unconnected.remove(&addr);
        self.connected.remove(&addr);
        self.attempts.remove(&addr);
        self.goods.remove(&addr);
        self.failures.insert(addr, local_now());
        self.update_net_size();
    }


    #[inline]
    fn try_connect(&mut self, addr: SocketAddr) {
        self.attempts.insert(addr, local_now());
        if let Err(_err) = self.dial.dial(addr) {
            self.attempts.remove(&addr);
        }
    }
}


impl<Dial: crate::Dial> DiscoveryV1<Dial> {
    // addrs should be service address list
    pub fn back_fill(&mut self, addrs: Vec<SocketAddr>) {
        for addr in addrs {
            if self.failures.contains_key(&addr) ||
                self.connected.contains_key(&addr) ||
                self.unconnected.get(&addr).is_some_and(|d| d.remain_times > 0) {
                continue;
            }
            self.add_unconnected(addr);
        }
    }

    pub fn request_remotes(&mut self, nr_remotes: u32) {
        let outstanding = self.attempts.len() as u32;
        if outstanding >= nr_remotes {
            return;
        }

        for _ in 0..(nr_remotes - outstanding) {
            let next = self.unconnected.iter()
                .find(|(k, _)| !self.connected.contains_key(k) && !self.attempts.contains_key(k))
                .map(|(k, _)| k.clone())
                .or_else(|| {
                    self.seeds.iter()
                        .find(|(_, seed)| seed.temporary() && !self.attempts.contains_key(&seed.addr))
                        .map(|(_, seed)| seed.addr.clone())
                });

            if let Some(next) = next {
                self.try_connect(next);
            }
        }
    }

    // `addr` is the local listen addr
    pub fn on_self(&mut self, addr: SocketAddr) {
        self.connected.remove(&addr);
        if let Some(seed) = self.seed_mut(&addr) {
            seed.state = Permanent;
        } else {
            self.add_failure(addr)
        }
    }

    // `addr` is client or server socket addr
    pub fn on_incoming(&mut self, addr: SocketAddr, stage: u32) {
        self.unconnected.remove(&addr);
        self.attempts.remove(&addr); // removed only if `peer` is server socket addr
        self.connected.entry(addr)
            .and_modify(|x| x.add_stage(stage))
            .or_insert(Connected::new(stage));
        self.update_net_size();
    }

    // `service` is the peer tcp-server socket addr
    pub fn on_good(&mut self, service: SocketAddr, peer: TcpPeer) {
        self.unconnected.remove(&service);
        self.failures.remove(&service);
        self.attempts.remove(&service);
        self.goods.insert(service, peer);
    }

    // `addr` may be client or server socket addr
    pub fn on_disconnected(&mut self, addr: &SocketAddr) {
        self.connected.remove(addr);
        if let Some(seed) = self.seed_mut(addr) { // if peer is server socket addr
            seed.state = Temporary;
        }

        let Some(peer) = self.goods.remove(addr) else { return; };
        if let Some(seed) = self.seed_mut(&peer.addr) { // if peer is client socket addr
            seed.state = Temporary;
        }
        self.back_fill(vec![peer.addr]); // back fill server socket addr
    }

    // `service` is the server socket addr
    pub fn on_failure(&mut self, service: SocketAddr) {
        if let Some(seed) = self.seed_mut(&service) {
            seed.state = Temporary;
        } else {
            let to_failures = self.unconnected.get_mut(&service)
                .filter(|v| v.remain_times > 0)
                .map(|v| {
                    v.remain_times -= 1;
                    v.remain_times as i32 <= 0
                });
            if to_failures.unwrap_or(true) {
                self.add_failure(service)
            }
        }
    }


    // `addr` may be client or server socket addr
    #[inline]
    pub fn get_connected(&self, addr: &SocketAddr) -> Option<&Connected> {
        self.connected.get(addr)
    }

    #[inline]
    pub fn discoveries(&self) -> Discoveries {
        Discoveries {
            net_size: self.net_size,
            fan_out: self.fan_out,
            unconnected: self.unconnected.len() as u32,
        }
    }
}

