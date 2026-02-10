//! Per-IP and per-method rate limiting using Token Bucket algorithm
//!
//! Lock-free implementation using `DashMap` for concurrent rate limiting.
//! Supports different rate limits for different RPC methods based on computational cost.

use dashmap::DashMap;
use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

/// Result of a rate limit check
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitCheckResult {
    /// Request is allowed
    Allowed,
    /// Request is blocked due to rate limiting
    Blocked,
    /// Rate limiting is disabled
    Disabled,
}

impl RateLimitCheckResult {
    /// Returns `true` if the request is allowed
    #[must_use]
    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed | Self::Disabled)
    }

    /// Returns `true` if the request is blocked
    #[must_use]
    pub const fn is_blocked(&self) -> bool {
        matches!(self, Self::Blocked)
    }
}

/// Rate limit tier for different method categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RateLimitTier {
    /// Cheap read-only operations (getblockcount, getversion, etc.)
    Cheap,
    /// Standard operations (getblock, gettransaction, etc.)
    Standard,
    /// Expensive operations (invokefunction, invokescript, findstorage, etc.)
    Expensive,
    /// Write operations (sendrawtransaction)
    Write,
}

impl RateLimitTier {
    /// Get the rate limit tier for a given RPC method
    #[must_use]
    pub fn from_method(method: &str) -> Self {
        let method_lower = method.to_ascii_lowercase();
        match method_lower.as_str() {
            // Cheap operations - simple state lookups
            "getblockcount" | "getconnectioncount" | "getrawmempool" | "getversion"
            | "getcommittee" | "getvalidators" | "ping" => Self::Cheap,

            // Expensive operations - VM execution, complex queries
            "invokefunction"
            | "invokescript"
            | "invokecontractverify"
            | "findstorage"
            | "findstates"
            | "traverseiterator"
            | "terminatesession"
            | "getunclaimedgas"
            | "calculatenetworkfee"
            | "getwalletunclaimedgas" => Self::Expensive,

            // Write operations
            "sendrawtransaction" | "submitblock" => Self::Write,

            // Standard operations - everything else
            _ => Self::Standard,
        }
    }

    /// Get the default rate limit configuration for this tier
    #[must_use]
    pub const fn default_config(&self) -> RateLimitConfig {
        match self {
            Self::Cheap => RateLimitConfig {
                max_rps: 200,
                burst: 400,
            },
            Self::Standard => RateLimitConfig {
                max_rps: 100,
                burst: 200,
            },
            Self::Expensive => RateLimitConfig {
                max_rps: 20,
                burst: 40,
            },
            Self::Write => RateLimitConfig {
                max_rps: 10,
                burst: 20,
            },
        }
    }
}

/// Configuration for rate limiting
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per second per IP
    pub max_rps: u32,
    /// Burst capacity (requests allowed in a short burst)
    pub burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_rps: 100,
            burst: 200,
        }
    }
}

/// Per-IP token bucket state for a specific tier
struct TokenBucket {
    /// Available tokens
    tokens: f64,
    /// Last refill timestamp
    last_refill: Instant,
}

/// Per-IP rate limit state across all tiers
struct IpRateLimitState {
    /// Buckets for each rate limit tier
    buckets: HashMap<RateLimitTier, TokenBucket>,
    /// Last access time for cleanup
    last_access: Instant,
}

impl IpRateLimitState {
    fn new() -> Self {
        Self {
            buckets: HashMap::new(),
            last_access: Instant::now(),
        }
    }

    fn get_or_create_bucket(
        &mut self,
        tier: RateLimitTier,
        config: &RateLimitConfig,
    ) -> &mut TokenBucket {
        self.buckets.entry(tier).or_insert_with(|| TokenBucket {
            tokens: config.burst as f64,
            last_refill: Instant::now(),
        })
    }
}

/// Lock-free per-IP rate limiter using `DashMap`
///
/// Uses token bucket algorithm with automatic cleanup of stale entries.
/// Supports per-method rate limiting with different tiers.
pub struct GovernorRateLimiter {
    default_config: RateLimitConfig,
    tier_configs: HashMap<RateLimitTier, RateLimitConfig>,
    states: DashMap<IpAddr, IpRateLimitState>,
    /// Whether rate limiting is enabled
    enabled: bool,
}

impl GovernorRateLimiter {
    /// Create a new rate limiter with the given configuration
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        let enabled = config.max_rps > 0;
        let mut tier_configs = HashMap::new();

        // Initialize tier configs based on default, scaled relative to the default config
        let scale_factor = if config.max_rps > 0 {
            config.max_rps as f64 / 100.0 // 100 is the default standard RPS
        } else {
            1.0
        };

        for tier in [
            RateLimitTier::Cheap,
            RateLimitTier::Standard,
            RateLimitTier::Expensive,
            RateLimitTier::Write,
        ] {
            let default_tier_config = tier.default_config();
            let tier_config = RateLimitConfig {
                max_rps: ((default_tier_config.max_rps as f64) * scale_factor) as u32,
                burst: ((default_tier_config.burst as f64) * scale_factor) as u32,
            };
            tier_configs.insert(tier, tier_config);
        }

        Self {
            default_config: config,
            tier_configs,
            states: DashMap::new(),
            enabled,
        }
    }

    /// Create a new rate limiter with per-tier configurations
    #[must_use]
    pub fn with_tier_configs(
        default_config: RateLimitConfig,
        tier_configs: HashMap<RateLimitTier, RateLimitConfig>,
    ) -> Self {
        Self {
            default_config: default_config.clone(),
            tier_configs,
            states: DashMap::new(),
            enabled: default_config.max_rps > 0,
        }
    }

    /// Check if a request from the given IP should be allowed
    ///
    /// Returns `RateLimitCheckResult::Allowed` if the request is allowed,
    /// `RateLimitCheckResult::Blocked` if rate limited.
    #[must_use]
    pub fn check(&self, ip: IpAddr) -> RateLimitCheckResult {
        self.check_with_tier(ip, RateLimitTier::Standard)
    }

    /// Check if a request from the given IP should be allowed for a specific method
    ///
    /// Returns `RateLimitCheckResult::Allowed` if the request is allowed,
    /// `RateLimitCheckResult::Blocked` if rate limited.
    #[must_use]
    pub fn check_for_method(&self, ip: IpAddr, method: &str) -> RateLimitCheckResult {
        let tier = RateLimitTier::from_method(method);
        self.check_with_tier(ip, tier)
    }

    /// Check if a request from the given IP should be allowed for a specific tier
    ///
    /// Returns `RateLimitCheckResult::Allowed` if the request is allowed,
    /// `RateLimitCheckResult::Blocked` if rate limited.
    #[must_use]
    pub fn check_with_tier(&self, ip: IpAddr, tier: RateLimitTier) -> RateLimitCheckResult {
        // Return early if rate limiting is disabled
        if !self.enabled {
            return RateLimitCheckResult::Disabled;
        }

        // Cleanup stale entries periodically
        self.cleanup_stale_entries();

        let config = self.tier_configs.get(&tier).cloned().unwrap_or_else(|| {
            // Fallback to default config scaled appropriately
            match tier {
                RateLimitTier::Cheap => RateLimitConfig {
                    max_rps: self.default_config.max_rps * 2,
                    burst: self.default_config.burst * 2,
                },
                RateLimitTier::Standard => self.default_config.clone(),
                RateLimitTier::Expensive => RateLimitConfig {
                    max_rps: self.default_config.max_rps / 5,
                    burst: self.default_config.burst / 5,
                },
                RateLimitTier::Write => RateLimitConfig {
                    max_rps: self.default_config.max_rps / 10,
                    burst: self.default_config.burst / 10,
                },
            }
        });

        let max_rps = f64::from(config.max_rps);
        let burst = f64::from(config.burst.max(1));

        // Get or create state for this IP
        let mut entry = self.states.entry(ip).or_insert_with(IpRateLimitState::new);

        // Update last access time
        entry.last_access = Instant::now();

        // Get or create bucket for this tier
        let bucket = entry.get_or_create_bucket(tier, &config);
        let now = Instant::now();

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            bucket.tokens = elapsed.mul_add(max_rps, bucket.tokens).min(burst);
            bucket.last_refill = now;
        }

        // Check if we have tokens available
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            RateLimitCheckResult::Allowed
        } else {
            RateLimitCheckResult::Blocked
        }
    }

    /// Check rate limit and return a result that must be used
    ///
    /// This version ensures the caller cannot accidentally ignore the result.
    #[must_use]
    pub fn check_required(&self, ip: IpAddr, method: Option<&str>) -> RateLimitCheckResult {
        match method {
            Some(m) => self.check_for_method(ip, m),
            None => self.check(ip),
        }
    }

    /// Remove entries that haven't been accessed in 10 minutes
    fn cleanup_stale_entries(&self) {
        const STALE_AFTER: Duration = Duration::from_secs(10 * 60);
        const MAX_ENTRIES: usize = 4096;

        if self.states.len() <= MAX_ENTRIES {
            return;
        }

        let now = Instant::now();
        self.states
            .retain(|_, state| now.duration_since(state.last_access) < STALE_AFTER);
    }

    /// Get current number of tracked IPs
    #[must_use]
    pub fn tracked_ips(&self) -> usize {
        self.states.len()
    }

    /// Get the configuration for a specific tier
    #[must_use]
    pub fn tier_config(&self, tier: RateLimitTier) -> Option<RateLimitConfig> {
        self.tier_configs.get(&tier).cloned()
    }

    /// Returns true if rate limiting is enabled
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Default for GovernorRateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

/// Builder for creating a rate limiter with custom tier configurations
pub struct RateLimiterBuilder {
    default_config: RateLimitConfig,
    tier_configs: HashMap<RateLimitTier, RateLimitConfig>,
}

impl RateLimiterBuilder {
    /// Create a new builder with the given default configuration
    #[must_use]
    pub fn new(default_config: RateLimitConfig) -> Self {
        Self {
            default_config,
            tier_configs: HashMap::new(),
        }
    }

    /// Set the configuration for a specific tier
    #[must_use]
    pub fn with_tier_config(mut self, tier: RateLimitTier, config: RateLimitConfig) -> Self {
        self.tier_configs.insert(tier, config);
        self
    }

    /// Build the rate limiter
    #[must_use]
    pub fn build(self) -> GovernorRateLimiter {
        GovernorRateLimiter::with_tier_configs(self.default_config, self.tier_configs)
    }
}

impl Default for RateLimiterBuilder {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_allows_requests_within_limit() {
        let config = RateLimitConfig {
            max_rps: 10,
            burst: 10,
        };
        let limiter = GovernorRateLimiter::new(config);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Should allow burst requests
        for _ in 0..10 {
            assert!(limiter.check(ip).is_allowed());
        }
    }

    #[test]
    fn test_rate_limiter_blocks_after_burst() {
        // Use a very low rate to ensure blocking happens quickly
        let config = RateLimitConfig {
            max_rps: 1,
            burst: 2,
        };
        let limiter = GovernorRateLimiter::new(config);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // First two requests should be allowed (burst=2)
        assert!(limiter.check(ip).is_allowed());
        assert!(limiter.check(ip).is_allowed());

        // Third request should be blocked (burst exhausted, rate is 1/sec)
        let result = limiter.check(ip);
        assert!(
            result.is_blocked(),
            "Expected blocked after burst exhausted, got {:?}",
            result
        );
    }

    #[test]
    fn test_rate_limiter_disabled_when_zero() {
        let config = RateLimitConfig {
            max_rps: 0,
            burst: 0,
        };
        let limiter = GovernorRateLimiter::new(config);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Should always allow when disabled
        for _ in 0..1000 {
            assert!(limiter.check(ip).is_allowed());
        }
        assert_eq!(limiter.check(ip), RateLimitCheckResult::Disabled);
    }

    #[test]
    fn test_rate_limiter_tracks_different_ips() {
        let config = RateLimitConfig {
            max_rps: 5,
            burst: 5,
        };
        let limiter = GovernorRateLimiter::new(config);
        let ip1: IpAddr = "127.0.0.1".parse().unwrap();
        let ip2: IpAddr = "192.168.1.1".parse().unwrap();

        // Exhaust ip1's burst
        for _ in 0..5 {
            let _ = limiter.check(ip1);
        }

        // ip2 should still have its full burst
        for _ in 0..5 {
            assert!(limiter.check(ip2).is_allowed());
        }
    }

    #[test]
    fn test_per_method_rate_limiting_expensive() {
        let config = RateLimitConfig {
            max_rps: 100,
            burst: 100,
        };
        let limiter = GovernorRateLimiter::new(config.clone());
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Expensive method should have lower limits
        let expensive_config = limiter.tier_config(RateLimitTier::Expensive).unwrap();
        assert!(expensive_config.max_rps < config.max_rps);

        // Should be able to make fewer expensive requests than standard
        let mut allowed_expensive = 0;
        for _ in 0..100 {
            if limiter.check_for_method(ip, "invokefunction").is_allowed() {
                allowed_expensive += 1;
            } else {
                break;
            }
        }
        assert!(allowed_expensive < 100);
    }

    #[test]
    fn test_per_method_rate_limiting_cheap() {
        let config = RateLimitConfig {
            max_rps: 50,
            burst: 50,
        };
        let limiter = GovernorRateLimiter::new(config.clone());
        let _ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Cheap method should have higher limits
        let cheap_config = limiter.tier_config(RateLimitTier::Cheap).unwrap();
        assert!(cheap_config.max_rps >= config.max_rps);

        // Verify cheap method is categorized correctly
        assert_eq!(
            RateLimitTier::from_method("getblockcount"),
            RateLimitTier::Cheap
        );
        assert_eq!(
            RateLimitTier::from_method("getversion"),
            RateLimitTier::Cheap
        );
    }

    #[test]
    fn test_rate_limit_tier_categorization() {
        assert_eq!(
            RateLimitTier::from_method("getblockcount"),
            RateLimitTier::Cheap
        );
        assert_eq!(
            RateLimitTier::from_method("getversion"),
            RateLimitTier::Cheap
        );
        assert_eq!(
            RateLimitTier::from_method("invokefunction"),
            RateLimitTier::Expensive
        );
        assert_eq!(
            RateLimitTier::from_method("invokescript"),
            RateLimitTier::Expensive
        );
        assert_eq!(
            RateLimitTier::from_method("sendrawtransaction"),
            RateLimitTier::Write
        );
        assert_eq!(
            RateLimitTier::from_method("getblock"),
            RateLimitTier::Standard
        );
        assert_eq!(
            RateLimitTier::from_method("gettransaction"),
            RateLimitTier::Standard
        );
    }

    #[test]
    fn test_check_required_with_method() {
        let config = RateLimitConfig {
            max_rps: 10,
            burst: 10,
        };
        let limiter = GovernorRateLimiter::new(config);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Test with specific method
        let result = limiter.check_required(ip, Some("invokefunction"));
        assert!(result.is_allowed() || result == RateLimitCheckResult::Disabled);

        // Test without method (uses standard tier)
        let result = limiter.check_required(ip, None);
        assert!(result.is_allowed() || result == RateLimitCheckResult::Disabled);
    }

    #[test]
    fn test_is_enabled() {
        let enabled_limiter = GovernorRateLimiter::new(RateLimitConfig {
            max_rps: 10,
            burst: 10,
        });
        assert!(enabled_limiter.is_enabled());

        let disabled_limiter = GovernorRateLimiter::new(RateLimitConfig {
            max_rps: 0,
            burst: 0,
        });
        assert!(!disabled_limiter.is_enabled());
    }

    #[test]
    fn test_rate_limit_check_result_methods() {
        assert!(RateLimitCheckResult::Allowed.is_allowed());
        assert!(!RateLimitCheckResult::Allowed.is_blocked());

        assert!(!RateLimitCheckResult::Blocked.is_allowed());
        assert!(RateLimitCheckResult::Blocked.is_blocked());

        assert!(RateLimitCheckResult::Disabled.is_allowed());
        assert!(!RateLimitCheckResult::Disabled.is_blocked());
    }

    #[test]
    fn test_builder_pattern() {
        let limiter = RateLimiterBuilder::new(RateLimitConfig {
            max_rps: 100,
            burst: 200,
        })
        .with_tier_config(
            RateLimitTier::Expensive,
            RateLimitConfig {
                max_rps: 5,
                burst: 10,
            },
        )
        .build();

        let expensive_config = limiter.tier_config(RateLimitTier::Expensive).unwrap();
        assert_eq!(expensive_config.max_rps, 5);
        assert_eq!(expensive_config.burst, 10);
    }
}
