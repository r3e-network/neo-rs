use actix::prelude::*;
use tokio::sync::{RwLock, mpsc};
use tokio::net::{TcpListener, TcpStream};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, SystemTime};
use std::sync::Arc;
use getset::{Getters, Setters};
use rand::prelude::IteratorRandom;
use serde::{Serialize, Deserialize};
use crate::network::{ChannelsConfig, RemoteNode};

pub trait Peer: Actor {
    const DEFAULT_MIN_DESIRED_CONNECTIONS: usize = 10;
    const DEFAULT_MAX_CONNECTIONS: usize = Self::DEFAULT_MIN_DESIRED_CONNECTIONS * 4;

    fn tcp_listener(&self) -> Option<Addr<TcpListener>>;
    fn set_tcp_listener(&mut self, listener: Option<Addr<TcpListener>>);
    fn timer(&self) -> Option<SpawnHandle>;
    fn set_timer(&mut self, timer: Option<SpawnHandle>);
    fn connected_addresses(&self) -> Arc<RwLock<HashMap<IpAddr, usize>>>;
    fn connected_peers(&self) -> Arc<RwLock<HashMap<Addr<RemoteNode>, SocketAddr>>>;
    fn unconnected_peers(&self) -> Arc<RwLock<HashSet<SocketAddr>>>;
    fn connecting_peers(&self) -> Arc<RwLock<HashSet<SocketAddr>>>;
    fn trusted_ip_addresses(&self) -> Arc<RwLock<HashSet<IpAddr>>>;
    fn listener_tcp_port(&self) -> u16;
    fn set_listener_tcp_port(&mut self, port: u16);
    fn max_connections_per_address(&self) -> usize;
    fn set_max_connections_per_address(&mut self, max: usize);
    fn min_desired_connections(&self) -> usize;
    fn set_min_desired_connections(&mut self, min: usize);
    fn max_connections(&self) -> usize;
    fn set_max_connections(&mut self, max: usize);
    fn unconnected_max(&self) -> usize;

    async fn connecting_max(&self) -> usize {
        let allowed_connecting = self.min_desired_connections() * 4;
        let max_connections = self.max_connections();
        if max_connections != usize::MAX && allowed_connecting > max_connections {
            max_connections
        } else {
            allowed_connecting
        }
        .saturating_sub(self.connected_peers().read().await.len())
    }

    async fn add_peers(&self, peers: Vec<SocketAddr>) {
        let mut unconnected = self.unconnected_peers().write().await;
        if unconnected.len() < self.unconnected_max() {
            let connected_peers = self.connected_peers().read().await;
            for peer in peers {
                if peer.port() != self.listener_tcp_port() || !is_local_address(&peer.ip()) {
                    if !connected_peers.values().any(|&addr| addr == peer) {
                        unconnected.insert(peer);
                        if unconnected.len() >= self.unconnected_max() {
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn connect_to_peer(&self, endpoint: SocketAddr, is_trusted: bool) {
        let endpoint = unmap_endpoint(endpoint);
        if endpoint.port() == self.listener_tcp_port() && is_local_address(&endpoint.ip()) {
            return;
        }

        if is_trusted {
            self.trusted_ip_addresses().write().await.insert(endpoint.ip());
        }

        let connected_count = self.connected_addresses().read().await.get(&endpoint.ip()).copied().unwrap_or(0);
        if connected_count >= self.max_connections_per_address() {
            return;
        }

        if self.connected_peers().read().await.values().any(|&addr| addr == endpoint) {
            return;
        }

        let mut connecting_peers = self.connecting_peers().write().await;
        if connecting_peers.len() < self.connecting_max() || is_trusted {
            if !connecting_peers.contains(&endpoint) {
                connecting_peers.insert(endpoint);
                // Initiate TCP connection (implementation depends on your network stack)
                self.initiate_tcp_connection(endpoint);
            }
        }
    }

    fn need_more_peers(&self, count: usize);

    fn on_start(&mut self, config: ChannelsConfig, ctx: &mut <Self as Actor>::Context) {
        self.set_listener_tcp_port(config.tcp.map_or(0, |tcp| tcp.port));
        self.set_min_desired_connections(config.min_desired_connections);
        self.set_max_connections(config.max_connections);
        self.set_max_connections_per_address(config.max_connections_per_address);

        self.set_timer(Some(ctx.run_interval(Duration::from_secs(5), |_, ctx| {
            ctx.address().do_send(PeerMessage::Timer);
        })));

        if self.listener_tcp_port() > 0 {
            // Bind TCP listener (implementation depends on your network stack)
            self.bind_tcp_listener(config.tcp);
        }
    }

    async fn on_tcp_connected(&mut self, remote: SocketAddr, local: SocketAddr) {
        self.connecting_peers().write().await.remove(&remote);

        if self.max_connections() != usize::MAX && 
           self.connected_peers().read().await.len() >= self.max_connections() && 
           !self.trusted_ip_addresses().read().await.contains(&remote.ip()) {
            // Abort connection
            return;
        }

        let mut connected_addresses = self.connected_addresses().write().await;
        let count = connected_addresses.entry(remote.ip()).or_insert(0);
        if *count >= self.max_connections_per_address() {
            // Abort connection
        } else {
            *count += 1;
            // Create and watch RemoteNode actor
            let remote_node = RemoteNode::new(/* connection details */);
            let remote_node_addr = remote_node.start();
            ctx.watch(remote_node_addr.clone());
            self.connected_peers().write().await.insert(remote_node_addr, remote);
            self.on_tcp_connected_impl(remote_node_addr);
        }
    }

    fn on_tcp_connected_impl(&mut self, connection: Addr<RemoteNode>) {
        // Default implementation, can be overridden
    }

    async fn on_tcp_command_failed(&mut self, cmd: TcpCommand) {
        match cmd {
            TcpCommand::Connect(addr) => {
                self.connecting_peers().write().await.remove(&addr);
            }
            // Handle other TCP command failures if needed
        }
    }

    async fn on_terminated(&mut self, actor: Addr<RemoteNode>) {
        if let Some(endpoint) = self.connected_peers().write().await.remove(&actor) {
            let mut connected_addresses = self.connected_addresses().write().await;
            if let Some(count) = connected_addresses.get_mut(&endpoint.ip()) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    connected_addresses.remove(&endpoint.ip());
                }
            }
        }
    }

    async fn on_timer(&mut self) {
        if self.connected_peers().read().await.len() >= self.min_desired_connections() {
            return;
        }

        if self.unconnected_peers().read().await.is_empty() {
            self.need_more_peers(self.min_desired_connections() - self.connected_peers().read().await.len());
        }

        let endpoints: Vec<SocketAddr> = {
            let mut rng = rand::thread_rng();
            self.unconnected_peers().write().await
                .drain()
                .choose_multiple(&mut rng, self.min_desired_connections() - self.connected_peers().read().await.len())
        };

        for endpoint in endpoints {
            self.connect_to_peer(endpoint, false);
        }
    }

    // Helper functions (implement these based on your specific network stack)
    fn initiate_tcp_connection(&self, endpoint: SocketAddr);
    fn bind_tcp_listener(&self, config: TcpConfig);
}

#[derive(Message)]
#[rtype(result = "()")]
pub enum PeerMessage {
    ChannelsConfig(ChannelsConfig),
    Timer,
    Peers { endpoints: Vec<SocketAddr> },
    Connect { endpoint: SocketAddr, is_trusted: bool },
    TcpConnected { remote: SocketAddr, local: SocketAddr },
    TcpBound,
    TcpCommandFailed(SocketAddr),
    Terminated(Addr<RemoteNode>),
}

fn is_local_address(addr: &IpAddr) -> bool {
    // Implement logic to check if the address is local
    unimplemented!()
}

fn unmap_endpoint(endpoint: SocketAddr) -> SocketAddr {
    // Implement logic to unmap IPv4-mapped IPv6 addresses
    unimplemented!()
}

#[derive(Debug)]
pub enum TcpCommand {
    Connect(SocketAddr),
    // Add other TCP commands as needed
}

pub struct TcpConfig {
    // Add TCP configuration fields
}
