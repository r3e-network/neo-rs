// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::net::SocketAddr;

use chrono::TimeDelta;
use crossbeam::atomic::AtomicCell;
use tokio::runtime::{self, Runtime};
use tokio::sync::mpsc;
use tokio_util::sync::{CancellationToken, DropGuard};

use crate::*;


pub struct InnerNode {
    runtime: Option<Runtime>,

    // ledger: Arc<dyn Ledger>,
    msg_handle: AtomicCell<Option<MessageHandle>>,
    driver: NetDriver,
    local: SocketAddr,
    settings: NodeSettings,
}

impl InnerNode {
    pub fn new(/* ledger: Arc<dyn Ledger>,*/ settings: NodeSettings) -> Self {
        let local: SocketAddr = settings.listen.parse()
            .expect(&format!("SocketAddr::parse({}) is not ok", &settings.listen));

        // let n = std::thread::available_parallelism().unwrap_or(8.into()).get();
        let runtime = runtime::Builder::new_multi_thread()
            .thread_name("p2p-worker")
            .enable_all()
            .build()
            .expect("`runtime::Builder` should be ok");

        let max_peers = settings.max_peers as usize;
        let mhs = settings.message_handle_settings(local.port());

        let (net_tx, net_rx) = mpsc::channel(MESSAGE_CHAN_SIZE);
        let driver = NetDriver::new(runtime.handle().clone(), max_peers, local, net_tx);
        Self {
            runtime: Some(runtime),
            msg_handle: AtomicCell::new(Some(MessageHandle::new(mhs, driver.handles(), net_rx))),
            driver,
            local,
            settings,
        }
    }

    #[inline]
    pub fn seeds(&self) -> &[String] { &self.settings.seeds }

    #[inline]
    pub fn port(&self) -> u16 { self.local.port() }

    #[inline]
    pub fn local_addr(&self) -> SocketAddr { self.local }

    // drop(NodeHandle) will close the listener
    pub fn run(&self) -> NodeHandle {
        let canceler = CancellationToken::new();
        let cancelee = canceler.clone();

        let (connect_tx, connect_rx) = mpsc::channel(CONNECT_CHAN_SIZE);
        let node = NodeHandle::new(connect_tx, canceler, self.seeds());

        let Some(handle) = self.msg_handle.take() else {
            return node;
        };
        let discovery = node.discovery();
        std::thread::spawn(move || handle.on_received(discovery));

        self.driver.on_connecting(connect_rx);
        self.driver.on_accepting(cancelee);

        self.on_discovering(node.discovery());
        node
    }

    pub fn on_discovering(&self, discovery: SharedDiscovery) {
        let attempt_peers = self.settings.attempt_peers;
        let per_block_millis = self.settings.per_block_millis;

        let check_millis = per_block_millis;
        let factor = self.settings.discovery_factor as i64;
        let broadcast = TimeDelta::milliseconds(factor * per_block_millis as i64);

        let Some(runtime) = self.runtime.as_ref() else { return; };
        let _discovering = runtime.spawn(async move {
            let mut broadcast_at = local_now();
            let mut ticker = tokio::time::interval(Duration::from_millis(check_millis));
            loop {
                let now = local_now();
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

impl Drop for InnerNode {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_timeout(Duration::from_secs(30));
        }
    }
}


#[allow(dead_code)]
pub struct NodeHandle {
    connect_tx: mpsc::Sender<SocketAddr>,
    discovery: SharedDiscovery,
    cancel: DropGuard,
}

impl NodeHandle {
    pub fn new(connect_tx: mpsc::Sender<SocketAddr>, cancel: CancellationToken, seeds: &[String]) -> Self {
        let resolver = DnsResolver::new(seeds);
        Self {
            connect_tx: connect_tx.clone(),
            discovery: Discovery::with_shared(connect_tx, resolver),
            cancel: cancel.drop_guard(),
        }
    }

    #[inline]
    pub fn discovery(&self) -> SharedDiscovery { self.discovery.clone() }
}


#[cfg(test)]
mod test {
    use std::{io::Write, net::TcpStream};
    use neo_core::payload::{P2pMessage, Ping};
    use super::*;

    #[test]
    fn test_run_node() {
        let node = InnerNode::new(NodeSettings::default());
        let addr = node.local_addr();

        let handle = node.run();
        std::thread::sleep(Duration::from_secs(1));

        let mut stream = TcpStream::connect(addr)
            .expect("`TcpStream::connect` should be ok");

        let ping = P2pMessage::Ping(Ping { last_block_index: 2, unix_seconds: 3, nonce: 4 });
        let buf = ping.to_message_encoded()
            .expect("`to_message_encoded` should be ok");

        stream.write_all(buf.as_ref())
            .expect("`write_all` should be ok");

        std::thread::sleep(Duration::from_secs(1));
        drop(handle);
    }
}