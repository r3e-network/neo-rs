use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;
use std::thread;
use rand::Rng;
use rand::rngs::ThreadRng;
use crate::network::capability::Capabilities;
use crate::network::Transporter;

const MAX_POOL_SIZE: usize = 10000;
const CONN_RETRIES: i32 = 3;
const TRY_MAX_WAIT: Duration = Duration::from_secs(1) / 2;

pub trait Discoverer {
    fn backfill(&self, addrs: Vec<String>);
    fn get_fan_out(&self) -> i32;
    fn network_size(&self) -> i32;
    fn pool_count(&self) -> usize;
    fn request_remote(&self, n: i32);
    fn register_self(&self, peer: &dyn AddressablePeer);
    fn register_good(&self, peer: &dyn AddressablePeer);
    fn register_connected(&self, peer: &dyn AddressablePeer);
    fn unregister_connected(&self, peer: &dyn AddressablePeer, duplicate: bool);
    fn unconnected_peers(&self) -> Vec<String>;
    fn bad_peers(&self) -> Vec<String>;
    fn good_peers(&self) -> Vec<AddressWithCapabilities>;
}

pub struct AddressWithCapabilities {
    pub address: String,
    pub capabilities: Capabilities,
}

pub struct DefaultDiscovery {
    seeds: HashMap<String, String>,
    transport: Arc<dyn Transporter>,
    lock: RwLock<()>,
    dial_timeout: Duration,
    bad_addrs: RwLock<HashMap<String, bool>>,
    connected_addrs: RwLock<HashMap<String, bool>>,
    handshaked_addrs: RwLock<HashMap<String, bool>>,
    good_addrs: RwLock<HashMap<String, Capabilities>>,
    unconnected_addrs: RwLock<HashMap<String, i32>>,
    attempted: RwLock<HashMap<String, bool>>,
    outstanding: AtomicI32,
    optimal_fan_out: AtomicI32,
    network_size: AtomicI32,
    request_ch: RwLock<Vec<i32>>,
}

impl DefaultDiscovery {
    pub fn new(addrs: Vec<String>, dt: Duration, ts: Arc<dyn Transporter>) -> Self {
        let seeds = addrs.into_iter().map(|addr| (addr, String::new())).collect();
        DefaultDiscovery {
            seeds,
            transport: ts,
            lock: RwLock::new(()),
            dial_timeout: dt,
            bad_addrs: RwLock::new(HashMap::new()),
            connected_addrs: RwLock::new(HashMap::new()),
            handshaked_addrs: RwLock::new(HashMap::new()),
            good_addrs: RwLock::new(HashMap::new()),
            unconnected_addrs: RwLock::new(HashMap::new()),
            attempted: RwLock::new(HashMap::new()),
            outstanding: AtomicI32::new(0),
            optimal_fan_out: AtomicI32::new(0),
            network_size: AtomicI32::new(0),
            request_ch: RwLock::new(Vec::new()),
        }
    }

    fn backfill(&self, addrs: Vec<String>) {
        let _lock = self.lock.write().unwrap();
        for addr in addrs {
            if self.bad_addrs.read().unwrap().contains_key(&addr)
                || self.connected_addrs.read().unwrap().contains_key(&addr)
                || self.handshaked_addrs.read().unwrap().contains_key(&addr)
                || self.unconnected_addrs.read().unwrap().get(&addr).unwrap_or(&0) > &0
            {
                continue;
            }
            self.push_to_pool_or_drop(addr);
        }
        self.update_net_size();
    }

    fn pool_count(&self) -> usize {
        self.unconnected_addrs.read().unwrap().len()
    }

    fn push_to_pool_or_drop(&self, addr: String) {
        if self.unconnected_addrs.read().unwrap().len() < MAX_POOL_SIZE {
            self.unconnected_addrs.write().unwrap().insert(addr, CONN_RETRIES);
        }
    }

    fn request_remote(&self, mut requested: i32) {
        let outstanding = self.outstanding.load(Ordering::SeqCst);
        requested -= outstanding;
        while requested > 0 {
            let mut next_addr = String::new();
            {
                let _lock = self.lock.write().unwrap();
                for addr in self.unconnected_addrs.read().unwrap().keys() {
                    if !self.connected_addrs.read().unwrap().contains_key(addr)
                        && !self.handshaked_addrs.read().unwrap().contains_key(addr)
                        && !self.attempted.read().unwrap().contains_key(addr)
                    {
                        next_addr = addr.clone();
                        break;
                    }
                }

                if next_addr.is_empty() {
                    for (addr, ip) in &self.seeds {
                        if ip.is_empty() && !self.attempted.read().unwrap().contains_key(addr) {
                            next_addr = addr.clone();
                            break;
                        }
                    }
                }

                if next_addr.is_empty() {
                    break;
                }

                self.attempted.write().unwrap().insert(next_addr.clone(), true);
            }
            self.outstanding.fetch_add(1, Ordering::SeqCst);
            let addr_clone = next_addr.clone();
            let transport_clone = Arc::clone(&self.transport);
            let dial_timeout_clone = self.dial_timeout;
            let self_clone = Arc::new(self);
            thread::spawn(move || {
                self_clone.try_address(addr_clone, transport_clone, dial_timeout_clone);
            });
            requested -= 1;
        }
    }

    fn register_self(&self, peer: &dyn AddressablePeer) {
        let connaddr = peer.connection_addr();
        let _lock = self.lock.write().unwrap();
        self.connected_addrs.write().unwrap().remove(&connaddr);
        self.register_bad(connaddr, true);
        self.register_bad(peer.peer_addr().to_string(), true);
    }

    fn register_bad(&self, addr: String, force: bool) {
        let is_seed = self.seeds.contains_key(&addr);
        if is_seed {
            if !force {
                self.seeds.insert(addr, String::new());
            } else {
                self.seeds.insert(addr, "forever".to_string());
            }
        } else {
            let mut unconnected_addrs = self.unconnected_addrs.write().unwrap();
            *unconnected_addrs.entry(addr.clone()).or_insert(0) -= 1;
            if unconnected_addrs[&addr] <= 0 || force {
                self.bad_addrs.write().unwrap().insert(addr.clone(), true);
                unconnected_addrs.remove(&addr);
                self.good_addrs.write().unwrap().remove(&addr);
            }
        }
        self.update_net_size();
    }

    fn unconnected_peers(&self) -> Vec<String> {
        self.unconnected_addrs.read().unwrap().keys().cloned().collect()
    }

    fn bad_peers(&self) -> Vec<String> {
        self.bad_addrs.read().unwrap().keys().cloned().collect()
    }

    fn good_peers(&self) -> Vec<AddressWithCapabilities> {
        self.good_addrs.read().unwrap().iter().map(|(addr, cap)| AddressWithCapabilities {
            address: addr.clone(),
            capabilities: cap.clone(),
        }).collect()
    }

    fn register_good(&self, peer: &dyn AddressablePeer) {
        let s = peer.peer_addr().to_string();
        let _lock = self.lock.write().unwrap();
        self.handshaked_addrs.write().unwrap().insert(s.clone(), true);
        self.good_addrs.write().unwrap().insert(s.clone(), peer.version().capabilities.clone());
        self.bad_addrs.write().unwrap().remove(&s);
    }

    fn unregister_connected(&self, peer: &dyn AddressablePeer, duplicate: bool) {
        let peeraddr = peer.peer_addr().to_string();
        let connaddr = peer.connection_addr();
        let _lock = self.lock.write().unwrap();
        self.connected_addrs.write().unwrap().remove(&connaddr);
        if !duplicate {
            for (addr, ip) in &self.seeds {
                if ip == &peeraddr {
                    self.seeds.insert(addr.clone(), String::new());
                    break;
                }
            }
            self.handshaked_addrs.write().unwrap().remove(&peeraddr);
            if self.good_addrs.read().unwrap().contains_key(&peeraddr) {
                self.backfill(vec![peeraddr]);
            }
        }
    }

    fn register_connected(&self, peer: &dyn AddressablePeer) {
        let addr = peer.connection_addr();
        let _lock = self.lock.write().unwrap();
        self.register_connected_addr(addr);
    }

    fn register_connected_addr(&self, addr: String) {
        self.unconnected_addrs.write().unwrap().remove(&addr);
        self.connected_addrs.write().unwrap().insert(addr, true);
        self.update_net_size();
    }

    fn get_fan_out(&self) -> i32 {
        self.optimal_fan_out.load(Ordering::SeqCst)
    }

    fn network_size(&self) -> i32 {
        self.network_size.load(Ordering::SeqCst)
    }

    fn update_net_size(&self) {
        let netsize = self.handshaked_addrs.read().unwrap().len() + self.unconnected_addrs.read().unwrap().len() + 1;
        let fan_out = if netsize == 2 {
            1.0
        } else {
            2.5 * (netsize as f64 - 1.0).ln()
        };
        self.optimal_fan_out.store((fan_out + 0.5) as i32, Ordering::SeqCst);
        self.network_size.store(netsize as i32, Ordering::SeqCst);
        update_network_size_metric(netsize);
        update_pool_count_metric(self.pool_count());
    }

    fn try_address(&self, addr: String, transport: Arc<dyn Transporter>, dial_timeout: Duration) {
        let mut rng = rand::thread_rng();
        let tout = rng.gen_range(0..TRY_MAX_WAIT.as_millis() as i64);
        thread::sleep(Duration::from_millis(tout as u64));
        let result = transport.dial(&addr, dial_timeout);
        self.outstanding.fetch_sub(1, Ordering::SeqCst);
        let _lock = self.lock.write().unwrap();
        self.attempted.write().unwrap().remove(&addr);
        match result {
            Ok(peer) => {
                if self.seeds.contains_key(&addr) {
                    self.seeds.insert(addr.clone(), peer.peer_addr().to_string());
                }
                self.register_connected_addr(addr);
            }
            Err(_) => {
                self.register_bad(addr.clone(), false);
                thread::sleep(dial_timeout);
                self.request_remote(1);
            }
        }
    }
}
