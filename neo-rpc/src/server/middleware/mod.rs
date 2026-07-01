//! # neo-rpc::server::middleware
//!
//! RPC middleware for transport-level policy and observability.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rate_limiter`: RPC rate-limiter middleware.

mod rate_limiter;

pub use rate_limiter::{
    GovernorRateLimiter, RateLimitCheckResult, RateLimitConfig, RateLimitTier, RateLimiterBuilder,
};
