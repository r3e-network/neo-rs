use std::net::{self, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use std::thread;
use std::panic;

use crate::network::capability;
use crate::network::payload;
use crate::network::discovery::{NewDefaultDiscovery, AddressablePeer, Transporter};
use assert2::{assert, let_assert};
use std::collections::HashSet;

struct FakeTransp {
    ret_false: AtomicI32,
    started: AtomicBool,
    closed: AtomicBool,
    dial_ch: std::sync::mpsc::Sender<String>,
    host: String,
    port: String,
}

struct FakeAPeer {
    addr: String,
    peer: String,
    version: Option<payload::Version>,
}

impl FakeAPeer {
    fn connection_addr(&self) -> &str {
        &self.addr
    }

    fn peer_addr(&self) -> net::SocketAddr {
        self.peer.to_socket_addrs().unwrap().next().unwrap()
    }

    fn version(&self) -> &Option<payload::Version> {
        &self.version
    }
}

impl FakeTransp {
    fn new(addr: &str) -> Self {
        let (host, port) = match addr.to_socket_addrs() {
            Ok(mut addrs) => {
                let addr = addrs.next().unwrap();
                (addr.ip().to_string(), addr.port().to_string())
            }
            Err(_) => ("".to_string(), "".to_string()),
        };
        let (tx, _rx) = channel();
        FakeTransp {
            ret_false: AtomicI32::new(0),
            started: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            dial_ch: tx,
            host,
            port,
        }
    }

    fn dial(&self, addr: &str, _timeout: Duration) -> Result<FakeAPeer, String> {
        if self.ret_false.load(Ordering::SeqCst) > 0 {
            return Err("smth bad happened".to_string());
        }
        self.dial_ch.send(addr.to_string()).unwrap();
        Ok(FakeAPeer {
            addr: addr.to_string(),
            peer: addr.to_string(),
            version: None,
        })
    }

    fn accept(&self) {
        if self.started.load(Ordering::SeqCst) {
            panic!("started twice");
        }
        self.host = "0.0.0.0".to_string();
        self.port = "42".to_string();
        self.started.store(true, Ordering::SeqCst);
    }

    fn proto(&self) -> &str {
        ""
    }

    fn host_port(&self) -> (&str, &str) {
        (&self.host, &self.port)
    }

    fn close(&self) {
        if self.closed.load(Ordering::SeqCst) {
            panic!("closed twice");
        }
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[test]
fn test_default_discoverer() {
    let (tx, rx) = channel();
    let ts = FakeTransp {
        ret_false: AtomicI32::new(0),
        started: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        dial_ch: tx,
        host: "".to_string(),
        port: "".to_string(),
    };
    let d = NewDefaultDiscovery::new(None, Duration::from_millis(62), ts);

    let try_max_wait = 1; // Don't waste time.
    let mut set1 = vec!["1.1.1.1:10333".to_string(), "2.2.2.2:10333".to_string()];
    set1.sort();

    // Added addresses should end up in the pool and in the unconnected set.
    // Done twice to check re-adding unconnected addresses, which should be
    // a no-op.
    for _ in 0..2 {
        d.back_fill(&set1);
        assert!(d.pool_count() == set1.len());
        let mut set1d = d.unconnected_peers();
        set1d.sort();
        assert!(d.good_peers().is_empty());
        assert!(d.bad_peers().is_empty());
        assert!(set1 == set1d);
    }
    assert!(d.get_fan_out() == 2);

    // Request should make goroutines dial our addresses draining the pool.
    d.request_remote(set1.len());
    let mut dialled = Vec::new();
    for _ in 0..set1.len() {
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(a) => {
                dialled.push(a.clone());
                d.register_connected(&FakeAPeer {
                    addr: a.clone(),
                    peer: a,
                    version: None,
                });
            }
            Err(_) => panic!("timeout expecting for transport dial"),
        }
    }
    assert!(d.unconnected_peers().is_empty());
    dialled.sort();
    assert!(d.pool_count() == 0);
    assert!(d.bad_peers().is_empty());
    assert!(d.good_peers().is_empty());
    assert!(set1 == dialled);

    // Registered good addresses should end up in appropriate set.
    for addr in &set1 {
        d.register_good(&FakeAPeer {
            addr: addr.clone(),
            peer: addr.clone(),
            version: Some(payload::Version {
                capabilities: vec![capability::Capability {
                    type_: capability::CapabilityType::FullNode,
                    data: capability::Node { start_height: 123 },
                }],
            }),
        });
    }
    let g_addr_with_cap = d.good_peers();
    let mut g_addrs = Vec::new();
    for addr in &g_addr_with_cap {
        assert!(addr.capabilities == vec![capability::Capability {
            type_: capability::CapabilityType::FullNode,
            data: capability::Node { start_height: 123 },
        }]);
        g_addrs.push(addr.address.clone());
    }
    g_addrs.sort();
    assert!(d.pool_count() == 0);
    assert!(d.unconnected_peers().is_empty());
    assert!(d.bad_peers().is_empty());
    assert!(set1 == g_addrs);

    // Re-adding connected addresses should be no-op.
    d.back_fill(&set1);
    assert!(d.unconnected_peers().is_empty());
    assert!(d.bad_peers().is_empty());
    assert!(d.good_peers().len() == set1.len());
    assert!(d.pool_count() == 0);

    // Unregistering connected should work.
    for addr in &set1 {
        d.unregister_connected(&FakeAPeer {
            addr: addr.clone(),
            peer: addr.clone(),
            version: None,
        }, false);
    }
    assert!(d.unconnected_peers().len() == 2); // They're re-added automatically.
    assert!(d.bad_peers().is_empty());
    assert!(d.good_peers().len() == set1.len());
    assert!(d.pool_count() == 2);

    // Now make Dial() fail and wait to see addresses in the bad list.
    ts.ret_false.store(1, Ordering::SeqCst);
    assert!(d.pool_count() == set1.len());
    let mut set1d = d.unconnected_peers();
    set1d.sort();
    assert!(d.bad_peers().is_empty());
    assert!(set1 == set1d);

    let mut dialled_bad = Vec::new();
    d.request_remote(set1.len());
    for _ in 0..conn_retries {
        for _ in 0..set1.len() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(a) => dialled_bad.push(a),
                Err(_) => panic!("timeout expecting for transport dial"),
            }
        }
    }
    assert!(d.pool_count() == 0);
    dialled_bad.sort();
    for i in 0..set1.len() {
        for j in 0..conn_retries {
            assert!(set1[i] == dialled_bad[i * conn_retries + j]);
        }
    }
    assert!(d.bad_peers().len() == set1.len());
    assert!(d.good_peers().is_empty());
    assert!(d.unconnected_peers().is_empty());

    // Re-adding bad addresses is a no-op.
    d.back_fill(&set1);
    assert!(d.unconnected_peers().is_empty());
    assert!(d.bad_peers().len() == set1.len());
    assert!(d.good_peers().is_empty());
    assert!(d.pool_count() == 0);
}

#[test]
fn test_seed_discovery() {
    let seeds = vec!["1.1.1.1:10333".to_string(), "2.2.2.2:10333".to_string()];
    let (tx, rx) = channel();
    let ts = FakeTransp {
        ret_false: AtomicI32::new(1),
        started: AtomicBool::new(false),
        closed: AtomicBool::new(false),
        dial_ch: tx,
        host: "".to_string(),
        port: "".to_string(),
    };
    let mut sorted_seeds = seeds.clone();
    sorted_seeds.sort();

    let d = NewDefaultDiscovery::new(Some(sorted_seeds.clone()), Duration::from_millis(100), ts);
    let try_max_wait = 1; // Don't waste time.

    d.request_remote(seeds.len());
    for _ in 0..conn_retries * 2 {
        for _ in 0..seeds.len() {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(_) => {}
                Err(_) => panic!("timeout expecting for transport dial"),
            }
        }
    }
}
