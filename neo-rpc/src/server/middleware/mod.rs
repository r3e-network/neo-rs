//! Tower middleware integration for RPC server
//!
//! Provides rate limiting (via Governor), compression (gzip), and request timeout.

mod rate_limiter;

pub use rate_limiter::{GovernorRateLimiter, RateLimitConfig};
