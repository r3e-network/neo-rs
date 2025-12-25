//! Per-IP rate limiting using Token Bucket algorithm
//!
//! Lock-free implementation using DashMap for concurrent per-IP rate limiting.

use dashmap::DashMap;
use std::{
    net::IpAddr,
    time::{Duration, Instant},
};

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

/// Per-IP token bucket state
struct TokenBucket {
    /// Available tokens
    tokens: f64,
    /// Last refill timestamp
    last_refill: Instant,
}

/// Lock-free per-IP rate limiter using DashMap
///
/// Uses token bucket algorithm with automatic cleanup of stale entries.
pub struct GovernorRateLimiter {
    config: RateLimitConfig,
    buckets: DashMap<IpAddr, TokenBucket>,
}

impl GovernorRateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: DashMap::new(),
        }
    }

    /// Check if a request from the given IP should be allowed
    ///
    /// Returns `true` if the request is allowed, `false` if rate limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        // Disabled if max_rps is 0
        if self.config.max_rps == 0 {
            return true;
        }

        // Cleanup stale entries periodically
        self.cleanup_stale_entries();

        let now = Instant::now();
        let max_rps = self.config.max_rps as f64;
        let burst = if self.config.burst == 0 {
            self.config.max_rps.max(1) as f64
        } else {
            self.config.burst.max(1) as f64
        };

        // Get or create bucket for this IP
        let mut entry = self.buckets.entry(ip).or_insert_with(|| TokenBucket {
            tokens: burst,
            last_refill: now,
        });

        // Refill tokens based on elapsed time
        let elapsed = now.duration_since(entry.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            entry.tokens = (entry.tokens + elapsed * max_rps).min(burst);
            entry.last_refill = now;
        }

        // Check if we have tokens available
        if entry.tokens >= 1.0 {
            entry.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Remove entries that haven't been accessed in 10 minutes
    fn cleanup_stale_entries(&self) {
        const STALE_AFTER: Duration = Duration::from_secs(10 * 60);
        const MAX_ENTRIES: usize = 4096;

        if self.buckets.len() <= MAX_ENTRIES {
            return;
        }

        let now = Instant::now();
        self.buckets
            .retain(|_, bucket| now.duration_since(bucket.last_refill) < STALE_AFTER);
    }

    /// Get current number of tracked IPs
    #[allow(dead_code)]
    pub fn tracked_ips(&self) -> usize {
        self.buckets.len()
    }
}

impl Default for GovernorRateLimiter {
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
            assert!(limiter.check(ip));
        }
    }

    #[test]
    fn test_rate_limiter_blocks_after_burst() {
        let config = RateLimitConfig {
            max_rps: 5,
            burst: 5,
        };
        let limiter = GovernorRateLimiter::new(config);
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        // Exhaust burst
        for _ in 0..5 {
            assert!(limiter.check(ip));
        }

        // Next request should be blocked
        assert!(!limiter.check(ip));
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
            assert!(limiter.check(ip));
        }
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
            limiter.check(ip1);
        }

        // ip2 should still have its full burst
        for _ in 0..5 {
            assert!(limiter.check(ip2));
        }
    }
}
