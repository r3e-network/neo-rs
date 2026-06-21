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
fn test_disabled_limiter_does_not_track_ips() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 0,
        burst: 0,
    });
    let ip: IpAddr = "127.0.0.1".parse().unwrap();

    assert_eq!(limiter.check(ip), RateLimitCheckResult::Disabled);
    assert_eq!(limiter.tracked_ips(), 0);
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
fn test_cleanup_removes_stale_tracked_ips_after_entry_limit() {
    let limiter = GovernorRateLimiter::new(RateLimitConfig {
        max_rps: 10,
        burst: 10,
    });
    let stale_at = Instant::now() - Duration::from_secs(11 * 60);
    for index in 0..=4096u32 {
        let ip = IpAddr::from([
            10,
            ((index >> 16) & 0xff) as u8,
            ((index >> 8) & 0xff) as u8,
            (index & 0xff) as u8,
        ]);
        limiter.last_access.insert(ip, stale_at);
    }

    let active_ip = IpAddr::from([192, 0, 2, 1]);
    assert_eq!(limiter.check(active_ip), RateLimitCheckResult::Allowed);

    assert_eq!(limiter.tracked_ips(), 1);
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
fn test_scaled_zero_tier_blocks_when_limiter_enabled() {
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
