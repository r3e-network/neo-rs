//! Core peer management primitives shared across the networking actors.
//!
//! This module mirrors the behaviour provided by the C# `Neo.Network.P2P.Peer`
//! base class.  The original implementation exposes a rich set of utilities for
//! tracking connected nodes, coordinating outbound connection attempts and
//! handling timer driven maintenance.  The Rust port below follows the same
//! design so higher level actors (such as [`LocalNodeActor`]) can delegate all
//! bookkeeping to this component while preserving Akka semantics.

use super::{
    channels_config::ChannelsConfig, local_node::RemoteNodeSnapshot, payloads::VersionPayload,
};
use crate::network::u_pn_p::UPnP;
use akka::{context::ActorContext, mailbox::Cancelable, ActorRef};
use if_addrs::get_if_addrs;
use rand::{seq::IteratorRandom, thread_rng};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use tracing::{debug, trace, warn};

/// Interval used for connection maintenance checks (matches the C# timer).
const TIMER_INTERVAL: Duration = Duration::from_millis(5_000);

/// Ensures that we always request enough peers from the seed list when the
/// connection pool is empty.  Mirrors C# `MaxCountFromSeedList`.
pub const MAX_COUNT_FROM_SEED_LIST: usize = 5;

/// Lightweight timer message delivered to peer actors.
#[derive(Debug, Clone, Copy, Default)]
pub struct PeerTimer;

/// Tracks per-peer connection metadata for currently connected nodes.
#[derive(Clone, Debug)]
pub struct ConnectedPeer {
    pub actor: ActorRef,
    pub endpoint: SocketAddr,
    pub remote_endpoint: SocketAddr,
    pub is_trusted: bool,
    pub established_at: Instant,
}

/// Internal state mirroring the C# `Peer` fields.
#[derive(Debug)]
pub struct PeerState {
    config: ChannelsConfig,
    listener_tcp_port: u16,
    connected_peers: HashMap<String, ConnectedPeer>,
    connected_addresses: HashMap<IpAddr, usize>,
    unconnected_peers: HashSet<SocketAddr>,
    connecting_peers: HashSet<SocketAddr>,
    trusted_ip_addresses: HashSet<IpAddr>,
    local_addresses: HashSet<IpAddr>,
    timer: Option<Cancelable>,
    upnp_configured: bool,
}

impl PeerState {
    /// Creates the peer state, seeding it with discovered local addresses so we
    /// can avoid self connections just like the C# implementation.
    pub fn new(initial_port: u16) -> Self {
        Self {
            config: ChannelsConfig::default(),
            listener_tcp_port: initial_port,
            connected_peers: HashMap::new(),
            connected_addresses: HashMap::new(),
            unconnected_peers: HashSet::new(),
            connecting_peers: HashSet::new(),
            trusted_ip_addresses: HashSet::new(),
            local_addresses: collect_local_addresses(),
            timer: None,
            upnp_configured: false,
        }
    }

    /// Applies the runtime configuration and starts the periodic maintenance
    /// timer exactly like `Peer.OnStart`.
    pub fn configure(&mut self, config: ChannelsConfig, ctx: &mut ActorContext) {
        self.config = config.clone();
        if let Some(endpoint) = config.tcp {
            self.listener_tcp_port = endpoint.port();
        }
        self.configure_upnp();
        self.ensure_timer(ctx);
    }

    /// Returns the active channel configuration.
    pub fn config(&self) -> &ChannelsConfig {
        &self.config
    }

    /// Cancels the scheduled maintenance timer.
    pub fn cancel_timer(&mut self) {
        if let Some(handle) = self.timer.take() {
            handle.cancel();
        }
    }

    /// Inserts new peers into the unconnected pool, applying the same
    /// filtering rules as the C# implementation.
    pub fn add_unconnected_peers<I>(&mut self, endpoints: I)
    where
        I: IntoIterator<Item = SocketAddr>,
    {
        if self.unconnected_peers.len() >= self.unconnected_max() {
            return;
        }

        for endpoint in endpoints {
            let endpoint = normalize_endpoint(endpoint);

            if (endpoint.port() == self.listener_tcp_port
                && self.local_addresses.contains(&endpoint.ip()))
                || self
                    .connected_peers
                    .values()
                    .any(|p| p.endpoint == endpoint || p.remote_endpoint == endpoint)
            {
                continue;
            }

            self.unconnected_peers.insert(endpoint);
        }
    }

    /// Marks the specified endpoint as being in the connecting state.  Returns
    /// `true` if the connection attempt should proceed.
    pub fn begin_connect(&mut self, endpoint: SocketAddr, is_trusted: bool) -> bool {
        let endpoint = normalize_endpoint(endpoint);

        if endpoint.port() == self.listener_tcp_port
            && self.local_addresses.contains(&endpoint.ip())
        {
            return false;
        }

        if !is_trusted && self.connected_peers.len() >= self.config.max_connections {
            return false;
        }

        let count = self
            .connected_addresses
            .get(&endpoint.ip())
            .cloned()
            .unwrap_or_default();
        if !is_trusted && count >= self.config.max_connections_per_address {
            return false;
        }

        if self
            .connected_peers
            .values()
            .any(|p| p.remote_endpoint == endpoint || p.endpoint == endpoint)
        {
            return false;
        }

        if !is_trusted && self.connecting_capacity() == 0 {
            return false;
        }

        if !self.connecting_peers.insert(endpoint) {
            return false;
        }

        if is_trusted {
            self.trusted_ip_addresses.insert(endpoint.ip());
        }

        true
    }

    /// Clears the connecting flag when an outbound attempt fails.
    pub fn connection_failed(&mut self, endpoint: SocketAddr) {
        let endpoint = normalize_endpoint(endpoint);
        self.connecting_peers.remove(&endpoint);
    }

    /// Registers a successful connection and updates per-address counters.  The
    /// caller is responsible for ensuring the connection has been authorised.
    pub fn register_connection(
        &mut self,
        actor: ActorRef,
        snapshot: &RemoteNodeSnapshot,
        is_trusted: bool,
        ctx: &mut ActorContext,
    ) {
        let remote_endpoint = normalize_endpoint(snapshot.remote_address);
        self.connecting_peers.remove(&remote_endpoint);

        if !is_trusted && self.connected_peers.len() >= self.config.max_connections {
            warn!(
                target: "neo", endpoint = %remote_endpoint,
                "connection limit reached while registering peer"
            );
            return;
        }

        if let Some(existing) = self
            .connected_peers
            .values()
            .find(|peer| {
                peer.remote_endpoint == remote_endpoint
                    || peer.endpoint == remote_endpoint
                    || (snapshot.listen_tcp_port != 0
                        && peer.endpoint.port() == snapshot.listen_tcp_port
                        && peer.endpoint.ip() == remote_endpoint.ip())
            })
            .cloned()
        {
            trace!(
                target: "neo",
                endpoint = %remote_endpoint,
                existing = %existing.actor,
                "duplicate peer connection ignored"
            );
            return;
        }

        let count = self
            .connected_addresses
            .entry(remote_endpoint.ip())
            .or_insert(0);
        *count += 1;

        if is_trusted {
            self.trusted_ip_addresses.insert(remote_endpoint.ip());
        }

        let path_key = actor.path().to_string();
        let advertised_endpoint = if snapshot.listen_tcp_port != 0
            && snapshot.listen_tcp_port != remote_endpoint.port()
        {
            SocketAddr::new(remote_endpoint.ip(), snapshot.listen_tcp_port)
        } else {
            remote_endpoint
        };
        self.connected_peers.insert(
            path_key,
            ConnectedPeer {
                actor: actor.clone(),
                endpoint: advertised_endpoint,
                remote_endpoint,
                is_trusted,
                established_at: Instant::now(),
            },
        );

        if let Err(error) = ctx.watch(&actor) {
            warn!(target: "neo", error = %error, "failed to watch remote node actor");
        }
    }

    /// Removes bookkeeping for the specified actor and returns the associated
    /// endpoint if it was tracked.
    pub fn unregister_connection(&mut self, actor: &ActorRef) -> Option<(SocketAddr, SocketAddr)> {
        let key = actor.path().to_string();
        if let Some(peer) = self.connected_peers.remove(&key) {
            let ip = peer.remote_endpoint.ip();
            if let Some(count) = self.connected_addresses.get_mut(&ip) {
                if *count > 1 {
                    *count -= 1;
                } else {
                    self.connected_addresses.remove(&ip);
                }
            }
            return Some((peer.endpoint, peer.remote_endpoint));
        }
        None
    }

    /// Number of active connections.
    pub fn connected_count(&self) -> usize {
        self.connected_peers.len()
    }

    /// Returns a list with all currently tracked remote endpoints.
    pub fn connected_endpoints(&self) -> Vec<SocketAddr> {
        self.connected_peers
            .values()
            .map(|peer| peer.endpoint)
            .collect()
    }

    /// Provides a snapshot of endpoints currently under connection attempts.
    pub fn connecting_endpoints(&self) -> Vec<SocketAddr> {
        self.connecting_peers.iter().copied().collect()
    }

    /// Determines how many additional connections are required to reach the
    /// configured minimum.
    pub fn connection_deficit(&self) -> usize {
        self.config
            .min_desired_connections
            .saturating_sub(self.connected_peers.len())
    }

    /// Selects a random subset of unconnected peers to attempt connecting to.
    pub fn take_connect_targets(&mut self, limit: usize) -> Vec<SocketAddr> {
        if limit == 0 || self.unconnected_peers.is_empty() {
            return Vec::new();
        }

        let mut rng = thread_rng();
        let selection: Vec<_> = self
            .unconnected_peers
            .iter()
            .copied()
            .choose_multiple(&mut rng, limit.min(self.unconnected_peers.len()));

        for endpoint in &selection {
            self.unconnected_peers.remove(endpoint);
        }

        selection
    }

    /// Indicates whether there are any queued unconnected peers.
    pub fn has_unconnected_peers(&self) -> bool {
        !self.unconnected_peers.is_empty()
    }

    fn unconnected_max(&self) -> usize {
        1_000
    }

    pub fn connecting_capacity(&self) -> usize {
        let mut allowed = self.config.min_desired_connections * 4;
        if self.config.max_connections > 0 && allowed > self.config.max_connections {
            allowed = self.config.max_connections;
        }
        allowed.saturating_sub(self.connected_peers.len() + self.connecting_peers.len())
    }

    fn ensure_timer(&mut self, ctx: &mut ActorContext) {
        self.cancel_timer();
        let handle = ctx.schedule_tell_repeatedly_cancelable(
            Duration::default(),
            TIMER_INTERVAL,
            &ctx.self_ref(),
            PeerTimer,
            None,
        );
        self.timer = Some(handle);
    }

    fn configure_upnp(&mut self) {
        if self.listener_tcp_port == 0 || self.upnp_configured {
            return;
        }

        let all_intranet = self
            .local_addresses
            .iter()
            .all(|addr| !is_ipv4_mapped(addr) || is_intranet_address(addr));

        if !all_intranet {
            return;
        }

        if !UPnP::discover() {
            return;
        }

        if let Some(ip) = UPnP::get_external_ip() {
            if let Ok(addr) = ip.parse::<IpAddr>() {
                self.local_addresses.insert(addr);
            }
        }

        let _ = UPnP::forward_port(self.listener_tcp_port as i32, "TCP", "NEO Tcp");
        self.upnp_configured = true;
    }
}

/// Messages handled by the peer controller.  These align with the public
/// commands exchanged with the C# actor hierarchy.
#[derive(Debug)]
pub enum PeerCommand {
    /// Applies the runtime configuration and starts the listener (equivalent to
    /// sending `ChannelsConfig` in C#).
    Configure { config: ChannelsConfig },
    /// Adds more endpoints to the unconnected pool.
    AddPeers { endpoints: Vec<SocketAddr> },
    /// Attempts to connect to the supplied endpoint.
    Connect {
        endpoint: SocketAddr,
        is_trusted: bool,
    },
    /// Registers a fully established remote node.
    ConnectionEstablished {
        actor: ActorRef,
        snapshot: RemoteNodeSnapshot,
        is_trusted: bool,
        inbound: bool,
        version: VersionPayload,
        reply: oneshot::Sender<bool>,
    },
    /// Removes the connecting flag after a failure.
    ConnectionFailed { endpoint: SocketAddr },
    /// Handles termination of a remote actor.
    ConnectionTerminated { actor: ActorRef },
    /// Triggered by the periodic maintenance timer.
    TimerElapsed,
    /// Returns the endpoints currently queued for connection.
    QueryConnectingPeers {
        reply: oneshot::Sender<Vec<SocketAddr>>,
    },
}

fn collect_local_addresses() -> HashSet<IpAddr> {
    let mut addresses = HashSet::new();
    match get_if_addrs() {
        Ok(iter) => {
            for iface in iter {
                addresses.insert(iface.ip());
            }
        }
        Err(err) => {
            warn!(target: "neo", error = %err, "failed to enumerate local interfaces");
        }
    }
    addresses
}

fn normalize_endpoint(endpoint: SocketAddr) -> SocketAddr {
    match endpoint {
        SocketAddr::V6(v6) => v6
            .ip()
            .to_ipv4()
            .map(|ipv4| SocketAddr::new(IpAddr::V4(ipv4), v6.port()))
            .unwrap_or(SocketAddr::V6(v6)),
        _ => endpoint,
    }
}

fn is_ipv4_mapped(addr: &IpAddr) -> bool {
    matches!(addr, IpAddr::V6(v6) if v6.to_ipv4().is_some())
}

fn is_intranet_address(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            let value = u32::from_be_bytes(octets);
            (value & 0xff00_0000) == 0x0a00_0000
                || (value & 0xff00_0000) == 0x7f00_0000
                || (value & 0xfff0_0000) == 0xac10_0000
                || (value & 0xffff_0000) == 0xc0a8_0000
                || (value & 0xffff_0000) == 0xa9fe_0000
        }
        IpAddr::V6(v6) => v6
            .to_ipv4()
            .map(|v4| is_intranet_address(&IpAddr::V4(v4)))
            .unwrap_or(false),
    }
}

impl Drop for PeerState {
    fn drop(&mut self) {
        if let Some(handle) = self.timer.take() {
            handle.cancel();
        }
    }
}
