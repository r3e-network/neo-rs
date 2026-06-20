//! Integration tests for the Governor-backed RPC rate limiter.
#![cfg(feature = "server")]

use neo_rpc::server::middleware::{
    GovernorRateLimiter, RateLimitCheckResult, RateLimitConfig, RateLimitTier, RateLimiterBuilder,
};
use std::net::IpAddr;

#[test]
fn governor_limiter_exhausts_configured_burst() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 1,
        burst: 2,
    });
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(limiter.check(ip), RateLimitCheckResult::Allowed);
    assert_eq!(limiter.check(ip), RateLimitCheckResult::Allowed);
    assert_eq!(limiter.check(ip), RateLimitCheckResult::Blocked);
}

#[test]
fn governor_limiter_keeps_per_ip_buckets_independent() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 1,
        burst: 1,
    });
    let first_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let second_ip: IpAddr = "127.0.0.2".parse().unwrap();

    assert_eq!(limiter.check(first_ip), RateLimitCheckResult::Allowed);
    assert_eq!(limiter.check(first_ip), RateLimitCheckResult::Blocked);
    assert_eq!(limiter.check(second_ip), RateLimitCheckResult::Allowed);
    assert_eq!(limiter.check(second_ip), RateLimitCheckResult::Blocked);
    assert_eq!(limiter.tracked_ips(), 2);
}

#[test]
fn governor_limiter_preserves_disabled_mode() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 0,
        burst: 0,
    });
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(limiter.check(ip), RateLimitCheckResult::Disabled);
    assert!(!limiter.is_enabled());
}

#[test]
fn governor_limiter_preserves_method_tiers() {
    assert_eq!(
        RateLimitTier::from_method("getblockcount"),
        RateLimitTier::Cheap
    );
    assert_eq!(
        RateLimitTier::from_method("invokefunction"),
        RateLimitTier::Expensive
    );
    assert_eq!(
        RateLimitTier::from_method("sendrawtransaction"),
        RateLimitTier::Write
    );

    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 100,
        burst: 100,
    });
    assert!(limiter.tier_config(RateLimitTier::Cheap).unwrap().max_rps > 100);
    assert_eq!(
        limiter.tier_config(RateLimitTier::Standard).unwrap().burst,
        100
    );
    assert!(
        limiter
            .tier_config(RateLimitTier::Expensive)
            .unwrap()
            .max_rps
            < 100
    );
}

#[test]
fn governor_limiter_blocks_scaled_zero_tiers() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 1,
        burst: 1,
    });
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(
        limiter.check_for_method(ip, "sendrawtransaction"),
        RateLimitCheckResult::Blocked
    );
    assert!(limiter.is_enabled());
}

#[test]
fn governor_limiter_keeps_tier_buckets_independent_per_ip() {
    let limiter = RateLimiterBuilder::new(RateLimitConfig {
        max_rps: 10,
        burst: 10,
    })
    .with_tier_config(
        RateLimitTier::Standard,
        RateLimitConfig {
            max_rps: 1,
            burst: 1,
        },
    )
    .with_tier_config(
        RateLimitTier::Expensive,
        RateLimitConfig {
            max_rps: 1,
            burst: 1,
        },
    )
    .build();
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(limiter.check(ip), RateLimitCheckResult::Allowed);
    assert_eq!(limiter.check(ip), RateLimitCheckResult::Blocked);
    assert_eq!(
        limiter.check_for_method(ip, "invokefunction"),
        RateLimitCheckResult::Allowed
    );
    assert_eq!(
        limiter.check_for_method(ip, "invokefunction"),
        RateLimitCheckResult::Blocked
    );
}

#[test]
fn governor_limiter_tracks_unique_ips_across_tiers() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 100,
        burst: 100,
    });
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(limiter.check(ip), RateLimitCheckResult::Allowed);
    assert_eq!(
        limiter.check_for_method(ip, "getblockcount"),
        RateLimitCheckResult::Allowed
    );
    assert_eq!(
        limiter.check_for_method(ip, "invokefunction"),
        RateLimitCheckResult::Allowed
    );
    assert_eq!(limiter.tracked_ips(), 1);
}

#[test]
fn governor_limiter_blocks_custom_zero_tier_without_disabling_limiter() {
    let limiter = RateLimiterBuilder::new(RateLimitConfig {
        max_rps: 10,
        burst: 10,
    })
    .with_tier_config(
        RateLimitTier::Cheap,
        RateLimitConfig {
            max_rps: 0,
            burst: 0,
        },
    )
    .build();
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert!(limiter.is_enabled());
    assert_eq!(
        limiter.check_for_method(ip, "getblockcount"),
        RateLimitCheckResult::Blocked
    );
    assert_eq!(limiter.check(ip), RateLimitCheckResult::Allowed);
}
