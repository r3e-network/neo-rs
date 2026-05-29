// Copyright (C) 2015-2025 The Neo Project.
//
// mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! P2P networking module matching C# `Neo.Network.P2P`.
//!
//! # Security Warning (H-6)
//!
//! **IMPORTANT**: P2P communications in this module are **NOT ENCRYPTED**.
//!
//! All network traffic between Neo nodes is transmitted in plaintext, which means:
//!
//! - **Eavesdropping**: Network observers can see all P2P messages including transactions,
//!   blocks, and consensus messages.
//! - **Man-in-the-Middle**: Attackers on the network path could potentially intercept and
//!   modify messages (though consensus signatures provide some protection).
//! - **Traffic Analysis**: Network patterns can reveal node behavior and relationships.
//!
//! ## Mitigations
//!
//! For production deployments, consider:
//!
//! 1. **VPN/Tunnel**: Run P2P traffic over an encrypted tunnel (WireGuard, IPsec)
//! 2. **Private Network**: Deploy nodes on isolated private networks
//! 3. **Tor/I2P**: Use anonymizing networks for additional privacy
//! 4. **Firewall Rules**: Restrict P2P connections to known trusted peers
//!
//! ## Why No Built-in Encryption?
//!
//! This matches the C# Neo reference implementation which also uses unencrypted TCP.
//! The Neo protocol relies on cryptographic signatures for message authenticity rather
//! than transport-layer encryption. Adding TLS would break compatibility with the
//! existing Neo network.
//!
//! ## Future Considerations
//!
//! A future protocol upgrade could add optional encryption (e.g., Noise Protocol Framework)
//! while maintaining backward compatibility through capability negotiation.

pub mod capabilities;
pub mod connection;
pub mod framed;
mod framed_codec;
pub mod helper;
#[cfg(feature = "runtime")]
pub mod local_node;
pub mod message;
pub mod messages;
pub mod payloads;
#[cfg(feature = "runtime")]
pub mod peer;
#[cfg(feature = "runtime")]
pub mod remote_node;
#[cfg(feature = "runtime")]
pub mod task_manager;
pub mod task_session;

// Re-export commonly used types
// ChannelsConfig + timeout counters are canonical in neo-p2p (low-level wire layer).
// neo-core's `network::p2p` acts as the facade mirroring C# `Neo.Network.P2P`,
// re-exporting the canonical neo-p2p submodules rather than duplicating them.
pub use neo_p2p::{channels_config, timeouts};
pub use neo_p2p::ChannelsConfig;
pub use connection::PeerConnection;
pub use framed::FrameConfig;
pub use helper::{get_sign_data, get_sign_data_vec};
#[cfg(feature = "runtime")]
pub use local_node::{
    BroadcastEvent, LocalNode, LocalNodeActor, LocalNodeCommand, LocalNodeHandle, RelayInventory,
    RemoteNodeSnapshot,
};
pub use message::Message;
// MessageCommand is the canonical P2P wire enum defined in neo-p2p; neo-core
// re-exports it rather than defining a byte-identical duplicate.
pub use neo_p2p::MessageCommand;
pub use neo_p2p::MessageFlags;
pub use messages::{MessageHeader, NetworkMessage, ProtocolMessage};
#[cfg(feature = "runtime")]
pub use peer::{ConnectedPeer, PeerCommand, PeerState, PeerTimer, MAX_COUNT_FROM_SEED_LIST};
#[cfg(feature = "runtime")]
pub use remote_node::{
    register_message_received_handler, unregister_message_received_handler,
    MessageHandlerSubscription, RemoteNode, RemoteNodeCommand,
};
#[cfg(feature = "runtime")]
pub use task_manager::{TaskManager, TaskManagerActor, TaskManagerCommand, TaskManagerHandle};
pub use task_session::TaskSession;

// Security hardening: Rate limiting and peer reputation constants
/// Default maximum inbound connections per second (token bucket rate limiter).
pub const DEFAULT_INBOUND_CONNECTION_RATE: usize = 10;

/// Default burst size for inbound connection rate limiter.
pub const DEFAULT_INBOUND_CONNECTION_BURST: usize = 20;

/// Default reputation threshold below which a peer is considered misbehaving.
pub const DEFAULT_REPUTATION_THRESHOLD: i32 = -100;

/// Default ban duration for misbehaving peers (24 hours).
pub const DEFAULT_BAN_DURATION: Duration = Duration::from_secs(86400);

/// Reputation score changes for various peer behaviors.
pub mod reputation {
    /// Reputation penalty for protocol violations.
    pub const PROTOCOL_VIOLATION: i32 = -50;
    /// Reputation penalty for sending invalid data.
    pub const INVALID_DATA: i32 = -30;
    /// Reputation penalty for connection failures.
    pub const CONNECTION_FAILURE: i32 = -10;
    /// Reputation reward for successful handshake.
    pub const SUCCESSFUL_HANDSHAKE: i32 = 10;
    /// Reputation reward for valid block relay.
    pub const VALID_BLOCK_RELAY: i32 = 5;
    /// Reputation reward for valid transaction relay.
    pub const VALID_TRANSACTION_RELAY: i32 = 1;
}

use governor::{
    middleware::StateInformationMiddleware, DefaultDirectRateLimiter, Quota, RateLimiter,
};
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Token bucket rate limiter for inbound connections.
#[derive(Debug)]
pub struct InboundRateLimiter {
    limiter: DefaultDirectRateLimiter<StateInformationMiddleware>,
    burst_size: usize,
    rate_per_sec: f64,
    last_available_tokens: f64,
    last_update: Instant,
    disabled: bool,
}

impl InboundRateLimiter {
    /// Creates a new rate limiter with the specified rate and burst size.
    pub fn new(rate_per_sec: usize, burst_size: usize) -> Self {
        let disabled = rate_per_sec == 0 || burst_size == 0;
        let quota = inbound_quota(rate_per_sec, burst_size);
        Self {
            limiter: RateLimiter::direct(quota).with_middleware::<StateInformationMiddleware>(),
            burst_size,
            rate_per_sec: rate_per_sec as f64,
            last_available_tokens: if disabled { 0.0 } else { burst_size as f64 },
            last_update: Instant::now(),
            disabled,
        }
    }

    /// Attempts to acquire a token. Returns true if a token was available.
    pub fn acquire(&mut self) -> bool {
        if self.disabled {
            return false;
        }

        self.last_update = Instant::now();
        match self.limiter.check() {
            Ok(snapshot) => {
                self.last_available_tokens = snapshot.remaining_burst_capacity() as f64;
                true
            }
            Err(_) => {
                self.last_available_tokens = 0.0;
                false
            }
        }
    }

    /// Returns the current number of available tokens.
    pub fn available_tokens(&self) -> f64 {
        if self.disabled {
            return 0.0;
        }

        let elapsed = Instant::now()
            .duration_since(self.last_update)
            .as_secs_f64();
        (self.last_available_tokens + elapsed * self.rate_per_sec).min(self.burst_size as f64)
    }
}

fn inbound_quota(rate_per_sec: usize, burst_size: usize) -> Quota {
    let rate = NonZeroU32::new(rate_per_sec.clamp(1, u32::MAX as usize) as u32)
        .expect("clamped rate is non-zero");
    let burst = NonZeroU32::new(burst_size.clamp(1, u32::MAX as usize) as u32)
        .expect("clamped burst is non-zero");
    Quota::per_second(rate).allow_burst(burst)
}

impl Default for InboundRateLimiter {
    fn default() -> Self {
        Self::new(
            DEFAULT_INBOUND_CONNECTION_RATE,
            DEFAULT_INBOUND_CONNECTION_BURST,
        )
    }
}

#[cfg(test)]
mod inbound_rate_limiter_tests {
    use super::InboundRateLimiter;

    #[test]
    fn inbound_rate_limiter_allows_initial_burst_then_rejects() {
        let mut limiter = InboundRateLimiter::new(10, 2);

        assert!(limiter.acquire());
        assert!(limiter.acquire());
        assert!(!limiter.acquire());
        assert!(limiter.available_tokens() < 1.0);
    }

    #[test]
    fn inbound_rate_limiter_zero_capacity_rejects() {
        let mut limiter = InboundRateLimiter::new(10, 0);

        assert!(!limiter.acquire());
        assert_eq!(limiter.available_tokens(), 0.0);
    }
}

/// Peer reputation tracking for identifying misbehaving peers.
#[derive(Debug, Clone)]
pub struct PeerReputation {
    /// Current reputation score.
    pub score: i32,
    /// Number of violations recorded.
    pub violations: u32,
    /// Last time the peer was seen.
    pub last_seen: Option<Instant>,
    /// Time when the peer was first seen.
    pub first_seen: Instant,
}

impl PeerReputation {
    /// Creates a new peer reputation with a neutral score.
    pub fn new() -> Self {
        Self {
            score: 0,
            violations: 0,
            last_seen: None,
            first_seen: Instant::now(),
        }
    }

    /// Adjusts the reputation score by the given delta.
    pub fn adjust_score(&mut self, delta: i32) {
        self.score = self.score.saturating_add(delta);
        if delta < 0 {
            self.violations += 1;
        }
        self.last_seen = Some(Instant::now());
    }

    /// Returns true if the peer's reputation is below the threshold.
    pub fn is_misbehaving(&self, threshold: i32) -> bool {
        self.score < threshold
    }

    /// Returns true if this is a new peer with no history.
    pub fn is_new(&self) -> bool {
        self.last_seen.is_none()
    }
}

crate::impl_default_via_new!(PeerReputation);

/// Ban list entry for a misbehaving peer.
#[derive(Debug, Clone)]
pub struct BanEntry {
    /// IP address of the banned peer.
    pub ip: IpAddr,
    /// Time when the ban was issued.
    pub banned_at: Instant,
    /// Duration of the ban.
    pub duration: Duration,
    /// Reason for the ban.
    pub reason: String,
}

impl BanEntry {
    /// Creates a new ban entry.
    pub fn new(ip: IpAddr, duration: Duration, reason: impl Into<String>) -> Self {
        Self {
            ip,
            banned_at: Instant::now(),
            duration,
            reason: reason.into(),
        }
    }

    /// Returns true if the ban has expired.
    pub fn is_expired(&self) -> bool {
        Instant::now().duration_since(self.banned_at) > self.duration
    }

    /// Returns the remaining time until the ban expires.
    pub fn remaining(&self) -> Duration {
        let elapsed = Instant::now().duration_since(self.banned_at);
        self.duration.saturating_sub(elapsed)
    }
}

/// Manages a list of banned peers with automatic expiration.
#[derive(Debug, Clone, Default)]
pub struct BanList {
    /// Banned IP addresses with their ban details.
    bans: std::collections::HashMap<IpAddr, BanEntry>,
}

impl BanList {
    /// Creates a new empty ban list.
    pub fn new() -> Self {
        Self {
            bans: std::collections::HashMap::new(),
        }
    }

    /// Bans a peer for the specified duration.
    pub fn ban(&mut self, ip: IpAddr, duration: Duration, reason: impl Into<String>) {
        let entry = BanEntry::new(ip, duration, reason);
        self.bans.insert(ip, entry);
    }

    /// Unbans a peer.
    pub fn unban(&mut self, ip: &IpAddr) -> bool {
        self.bans.remove(ip).is_some()
    }

    /// Returns true if the IP address is currently banned.
    pub fn is_banned(&self, ip: &IpAddr) -> bool {
        self.bans.get(ip).is_some_and(|entry| !entry.is_expired())
    }

    /// Returns the ban entry for an IP if it exists and is active.
    pub fn get_ban(&self, ip: &IpAddr) -> Option<&BanEntry> {
        self.bans.get(ip).filter(|entry| !entry.is_expired())
    }

    /// Removes expired bans and returns the count of removed entries.
    pub fn cleanup_expired(&mut self) -> usize {
        let before = self.bans.len();
        self.bans.retain(|_, entry| !entry.is_expired());
        before - self.bans.len()
    }

    /// Returns the number of active bans.
    pub fn active_ban_count(&self) -> usize {
        self.bans.values().filter(|e| !e.is_expired()).count()
    }

    /// Returns all active bans.
    pub fn active_bans(&self) -> Vec<&BanEntry> {
        self.bans.values().filter(|e| !e.is_expired()).collect()
    }
}

#[cfg(test)]
mod ban_list_tests {
    use super::{BanEntry, BanList};
    use std::net::{IpAddr, Ipv4Addr};
    use std::time::{Duration, Instant};

    fn test_ip(octet: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, octet))
    }

    fn expired_entry(ip: IpAddr) -> BanEntry {
        BanEntry {
            ip,
            banned_at: Instant::now() - Duration::from_secs(2),
            duration: Duration::from_secs(1),
            reason: "expired".to_string(),
        }
    }

    #[test]
    fn ban_list_reports_active_ban_and_metadata() {
        let mut bans = BanList::new();
        let ip = test_ip(1);

        bans.ban(ip, Duration::from_secs(60), "invalid message");

        let entry = bans.get_ban(&ip).expect("active ban entry");
        assert_eq!(entry.ip, ip);
        assert_eq!(entry.reason, "invalid message");
        assert!(entry.remaining() <= Duration::from_secs(60));
        assert!(bans.is_banned(&ip));
        assert_eq!(bans.active_ban_count(), 1);
    }

    #[test]
    fn ban_list_unban_removes_active_ban() {
        let mut bans = BanList::new();
        let ip = test_ip(2);

        bans.ban(ip, Duration::from_secs(60), "manual");

        assert!(bans.unban(&ip));
        assert!(!bans.unban(&ip));
        assert!(!bans.is_banned(&ip));
        assert_eq!(bans.active_ban_count(), 0);
    }

    #[test]
    fn ban_list_cleanup_expired_returns_removed_count() {
        let mut bans = BanList::new();
        let active = test_ip(3);
        let expired = test_ip(4);

        bans.ban(active, Duration::from_secs(60), "active");
        bans.bans.insert(expired, expired_entry(expired));

        assert!(!bans.is_banned(&expired));
        assert_eq!(bans.cleanup_expired(), 1);
        assert_eq!(bans.active_ban_count(), 1);
        assert!(bans.is_banned(&active));
    }

    #[test]
    fn ban_list_active_bans_exclude_expired_entries() {
        let mut bans = BanList::new();
        let active = test_ip(5);
        let expired = test_ip(6);

        bans.ban(active, Duration::from_secs(60), "active");
        bans.bans.insert(expired, expired_entry(expired));

        let active_bans = bans.active_bans();
        assert_eq!(active_bans.len(), 1);
        assert_eq!(active_bans[0].ip, active);
        assert_eq!(active_bans[0].reason, "active");
    }
}

/// Validates a peer endpoint before connection.
pub fn validate_peer_endpoint(endpoint: &std::net::SocketAddr) -> Result<(), &'static str> {
    let ip = endpoint.ip();

    // Reject unspecified addresses (0.0.0.0, ::)
    if ip.is_unspecified() {
        return Err("unspecified address not allowed");
    }

    // Reject multicast addresses
    if ip.is_multicast() {
        return Err("multicast address not allowed");
    }

    // Reject broadcast addresses (255.255.255.255)
    if let IpAddr::V4(v4) = ip {
        if v4.octets() == [255, 255, 255, 255] {
            return Err("broadcast address not allowed");
        }
    }

    // Reject port 0
    if endpoint.port() == 0 {
        return Err("port 0 not allowed");
    }

    // Reject port > 65535 (implicitly handled by u16)

    Ok(())
}

/// Shared peer reputation tracker for the P2P network.
#[derive(Debug, Default)]
pub struct PeerReputationTracker {
    /// Map of IP addresses to their reputation scores.
    reputations: RwLock<std::collections::HashMap<IpAddr, PeerReputation>>,
}

impl PeerReputationTracker {
    /// Creates a new reputation tracker.
    pub fn new() -> Self {
        Self {
            reputations: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Gets or creates the reputation for a peer.
    pub async fn get_reputation(&self, ip: IpAddr) -> PeerReputation {
        let reputations = self.reputations.read().await;
        reputations.get(&ip).cloned().unwrap_or_default()
    }

    /// Adjusts the reputation score for a peer.
    pub async fn adjust_reputation(&self, ip: IpAddr, delta: i32) {
        let mut reputations = self.reputations.write().await;
        reputations.entry(ip).or_default().adjust_score(delta);
    }

    /// Records a protocol violation for a peer.
    pub async fn record_violation(&self, ip: IpAddr, violation_type: &str) {
        let penalty = match violation_type {
            "invalid_message" => reputation::INVALID_DATA,
            "handshake_failure" => reputation::CONNECTION_FAILURE,
            "protocol_violation" => reputation::PROTOCOL_VIOLATION,
            _ => reputation::INVALID_DATA,
        };
        self.adjust_reputation(ip, penalty).await;
    }

    /// Records a successful contribution from a peer.
    pub async fn record_contribution(&self, ip: IpAddr, contribution_type: &str) {
        let reward = match contribution_type {
            "handshake_success" => reputation::SUCCESSFUL_HANDSHAKE,
            "valid_block" => reputation::VALID_BLOCK_RELAY,
            "valid_transaction" => reputation::VALID_TRANSACTION_RELAY,
            _ => 0,
        };
        if reward > 0 {
            self.adjust_reputation(ip, reward).await;
        }
    }

    /// Returns true if the peer is considered misbehaving.
    pub async fn is_misbehaving(&self, ip: IpAddr, threshold: i32) -> bool {
        let reputation = self.get_reputation(ip).await;
        reputation.is_misbehaving(threshold)
    }

    /// Cleans up old reputation entries and returns the count removed.
    pub async fn cleanup_old_entries(&self, max_age: Duration) -> usize {
        let mut reputations = self.reputations.write().await;
        let before = reputations.len();
        let now = Instant::now();
        reputations.retain(|_, rep| {
            rep.last_seen
                .map(|last| now.duration_since(last) < max_age)
                .unwrap_or(true)
        });
        before - reputations.len()
    }
}
