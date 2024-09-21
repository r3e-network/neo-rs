// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::net::SocketAddr;
use std::sync::Arc;

use crossbeam::atomic::AtomicCell;
use neo_base::time::{unix_millis_now, Tick};
use tokio::runtime::{self, Runtime};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::*;

pub struct LocalNode {
    runtime: Option<Runtime>,
    // ledger: Arc<dyn Ledger>,
    net_rx: AtomicCell<Option<mpsc::Receiver<NetMessage>>>,
    driver: NetDriver,
    local: SocketAddr,
    config: P2pConfig,
}

impl LocalNode {
    pub fn new(/* ledger: Arc<dyn Ledger>,*/ config: P2pConfig) -> Self {
        let local: SocketAddr = config
            .listen
            .parse()
            .expect(&format!("SocketAddr::parse({}) is not ok", &config.listen));

        // let n = std::thread::available_parallelism().unwrap_or(8.into()).get();
        let runtime = runtime::Builder::new_multi_thread()
            .thread_name("p2p-worker")
            .enable_all()
            .build()
            .expect("`runtime::Builder` should be ok");

        let handle = runtime.handle().clone();
        let (net_tx, net_rx) = mpsc::channel(MESSAGE_CHAN_SIZE);
        let driver = NetDriver::new(handle, config.max_peers as usize, local, net_tx);
        Self {
            runtime: Some(runtime),
            net_rx: AtomicCell::new(Some(net_rx)),
            driver,
            local,
            config,
        }
    }

    pub fn p2p_config(&self) -> &P2pConfig { &self.config }

    pub fn net_handles(&self) -> NetHandles { self.driver.net_handles() }

    pub fn seeds(&self) -> &[String] { &self.config.seeds }

    pub fn port(&self) -> u16 { self.local.port() }

    pub fn local_addr(&self) -> SocketAddr { self.local }

    // drop(NodeHandle) will close the listener
    pub fn run(&self, handle: MessageHandleV2) -> NodeHandle {
        let (connect_tx, connect_rx) = mpsc::channel(CONNECT_CHAN_SIZE);

        let tick = self.config.tick_interval;
        let node = NodeHandle::new(connect_tx, tick, self.seeds());

        let discovery = node.discovery();
        let to_recv = handle.clone();
        let Some(net_rx) = self.net_rx.take() else {
            panic!("`net_rx` has been token");
        };
        std::thread::spawn(move || {
            to_recv.on_received(net_rx, discovery);
        });

        let discovery = node.discovery();
        let tick = node.protocol_tick();
        std::thread::spawn(move || {
            handle.on_protocol_tick(tick, discovery);
        });

        self.driver.on_connecting(connect_rx);
        self.driver.on_accepting(node.cancelee());

        self.on_discovering(node.discovery());
        node
    }

    pub fn on_discovering(&self, discovery: Discovery) {
        let attempt_peers = self.config.attempt_peers;
        let per_block_millis = self.config.per_block_millis;

        let check_millis = per_block_millis;
        let factor = self.config.discovery_factor as u64;
        let broadcast = factor * per_block_millis;

        let Some(runtime) = self.runtime.as_ref() else {
            return;
        };
        let _discovering = runtime.spawn(async move {
            let mut broadcast_at = unix_millis_now();
            let mut ticker = tokio::time::interval(Duration::from_millis(check_millis));
            loop {
                let now = unix_millis_now();
                let stats = { discovery.lock().unwrap().discoveries() };
                if (now - broadcast_at) >= broadcast || stats.net_size < attempt_peers {
                    // TODO: broadcast the GetAddress message
                    broadcast_at = now;
                }

                let _ = ticker.tick().await;
            }
        });
    }
}

impl Drop for LocalNode {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_timeout(Duration::from_secs(30));
        }
    }
}

#[allow(dead_code)]
pub struct NodeHandle {
    // connect_tx: mpsc::Sender<SocketAddr>,
    discovery: Discovery,
    cancel: CancellationToken,
    tick: Arc<Tick>,
}

impl NodeHandle {
    pub fn new(
        connect_tx: mpsc::Sender<SocketAddr>,
        protocol_tick: Duration,
        seeds: &[String],
    ) -> Self {
        let resolver = DnsResolver::new(seeds);
        Self {
            // connect_tx: connect_tx.clone(),
            discovery: DiscoveryV1::with_shared(connect_tx, resolver),
            cancel: CancellationToken::new(),
            tick: Arc::new(Tick::new(protocol_tick)),
        }
    }

    fn protocol_tick(&self) -> Arc<Tick> { self.tick.clone() }

    pub(crate) fn cancelee(&self) -> CancellationToken { self.cancel.clone() }

    pub(crate) fn discovery(&self) -> Discovery { self.discovery.clone() }
}

impl Drop for NodeHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.tick.stop();
    }
}

#[cfg(test)]
mod test {
    use std::{io::Write, net::TcpStream};

    use neo_core::payload::{P2pMessage, Ping};

    use super::*;

    #[test]
    fn test_run_node() {
        let node = LocalNode::new(P2pConfig::default());
        let addr = node.local_addr();

        let message = MessageHandleV2::new(node.port(), node.config.clone(), node.net_handles());
        let handle = node.run(message);
        std::thread::sleep(Duration::from_secs(1));

        let mut stream = TcpStream::connect(addr)
            .expect("`TcpStream::connect` should be ok");

        let ping = P2pMessage::Ping(Ping { last_block_index: 2, unix_seconds: 3, nonce: 4 });
        let buf = ping.to_message_encoded()
            .expect("`to_message_encoded` should be ok");

        stream.write_all(buf.as_ref())
            .expect("`write_all` should be ok");
        std::thread::sleep(Duration::from_millis(200));

        drop(handle);
        std::thread::sleep(Duration::from_millis(500));
    }
}
