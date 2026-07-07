//! RPC-server rate-limit adapter.
//!
//! The generic governor limiter lives in `server::middleware`; this module is
//! the RPC-server-specific adapter that projects `RpcServerConfig` into that
//! limiter and maps blocked calls onto the Neo RPC error surface.

use std::net::{IpAddr, Ipv4Addr};

use crate::server::middleware::{GovernorRateLimiter, RateLimitCheckResult, RateLimitConfig};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_server_settings::RpcServerConfig;

use super::RpcServer;

impl RpcServer {
    /// Apply configured server-side rate limiting for one RPC method call.
    pub(crate) fn check_rate_limit(&self, method: &str) -> Result<(), RpcError> {
        match self
            .rate_limiter
            .check_for_method(global_rate_limit_key(), method)
        {
            RateLimitCheckResult::Allowed | RateLimitCheckResult::Disabled => Ok(()),
            RateLimitCheckResult::Blocked => Err(RpcError::too_many_requests()),
        }
    }
}

pub(super) fn rate_limiter_from_settings(settings: &RpcServerConfig) -> GovernorRateLimiter {
    GovernorRateLimiter::new(RateLimitConfig {
        max_rps: settings.max_requests_per_second,
        burst: settings.rate_limit_burst,
    })
}

fn global_rate_limit_key() -> IpAddr {
    IpAddr::V4(Ipv4Addr::UNSPECIFIED)
}
