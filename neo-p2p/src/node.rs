// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::net::SocketAddr;
use std::sync::Arc;

use crossbeam::atomic::AtomicCell;
use tokio::runtime::{self, Runtime};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use neo_base::time::{Tick, unix_millis_now};
use crate::*;


pub struct LocalNode {
    runtime: Option<Runtime>,
    // ledger: Arc<dyn Ledger>,
    net_rx: AtomicCell<Option<mpsc::Receiver<NetMessage>>>,
    driver: NetDriver,
    local: SocketAddr,
    settings: NodeSettings,
}

impl LocalNode {
    pub fn new(/* ledger: Arc<dyn Ledger>,*/ settings: NodeSettings) -> Self {
        let local: SocketAddr = settings.listen.parse()
            .expect(&format!("SocketAddr::parse({}) is not ok", &settings.listen));

        // let n = std::thread::available_parallelism().unwrap_or(8.into()).get();
        let runtime = runtime::Builder::new_multi_thread()
            .thread_name("p2p-worker")
            .enable_all()
            .build()
            .expect("`runtime::Builder` should be ok");

        let (net_tx, net_rx) = mpsc::channel(MESSAGE_CHAN_SIZE);
        let driver = NetDriver::new(
            runtime.handle().clone(),
            settings.max_peers as usize,
            local, net_tx,
        );
        Self {
            runtime: Some(runtime),
            net_rx: AtomicCell::new(Some(net_rx)),
            driver,
            local,
            settings,
        }
    }

    pub fn settings(&self) -> &NodeSettings { &self.settings }

    pub fn net_handles(&self) -> NetHandles { self.driver.net_handles() }

    pub fn seeds(&self) -> &[String] { &self.settings.seeds }

    pub fn port(&self) -> u16 { self.local.port() }

    pub fn local_addr(&self) -> SocketAddr { self.local }

    // drop(NodeHandle) will close the listener
    pub fn run(&self, handle: MessageHandleV2) -> NodeHandle {
        let (connect_tx, connect_rx) = mpsc::channel(CONNECT_CHAN_SIZE);

        let heartbeat = self.settings.ping_interval;
        let node = NodeHandle::new(connect_tx, heartbeat, self.seeds());

        let discovery = node.discovery();
        let to_recv = handle.clone();
        let Some(net_rx) = self.net_rx.take() else {
            return node;
        };
        std::thread::spawn(move || { to_recv.on_received(net_rx, discovery); });

        let discovery = node.discovery();
        let heartbeat = node.heartbeat_tick();
        std::thread::spawn(move || { handle.on_heartbeat(heartbeat, discovery); });

        self.driver.on_connecting(connect_rx);
        self.driver.on_accepting(node.cancelee());

        self.on_discovering(node.discovery());
        node
    }

    pub fn on_discovering(&self, discovery: Discovery) {
        let attempt_peers = self.settings.attempt_peers;
        let per_block_millis = self.settings.per_block_millis;

        let check_millis = per_block_millis;
        let factor = self.settings.discovery_factor as u64;
        let broadcast = factor * per_block_millis;

        let Some(runtime) = self.runtime.as_ref() else { return; };
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
    heartbeat_tick: Arc<Tick>,
}

impl NodeHandle {
    pub fn new(connect_tx: mpsc::Sender<SocketAddr>, heartbeat: Duration, seeds: &[String]) -> Self {
        let resolver = DnsResolver::new(seeds);
        Self {
            // connect_tx: connect_tx.clone(),
            discovery: DiscoveryV1::with_shared(connect_tx, resolver),
            cancel: CancellationToken::new(),
            heartbeat_tick: Arc::new(Tick::new(heartbeat)),
        }
    }

    fn heartbeat_tick(&self) -> Arc<Tick> { self.heartbeat_tick.clone() }

    pub fn cancelee(&self) -> CancellationToken { self.cancel.clone() }

    pub fn discovery(&self) -> Discovery { self.discovery.clone() }
}

impl Drop for NodeHandle {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.heartbeat_tick.stop();
    }
}


#[cfg(test)]
mod test {
    use std::{io::Write, net::TcpStream};
    use neo_core::payload::{P2pMessage, Ping};
    use super::*;

    #[test]
    fn test_run_node() {
        let node = LocalNode::new(NodeSettings::default());
        let addr = node.local_addr();

        let message = MessageHandleV2::new(
            node.settings.handle_settings(node.port()),
            node.net_handles(),
        );
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