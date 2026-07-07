//! Transport authentication helpers for the jsonrpsee adapter.
//!
//! The HTTP policy validates Basic credentials before dispatch and inserts
//! [`RpcAuthState`] into request extensions. The registered jsonrpsee method
//! callback then checks that marker before invoking protected RPC handlers.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use subtle::ConstantTimeEq;

/// Authentication marker inserted by the HTTP middleware after Basic auth succeeds.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RpcAuthState {
    authenticated: bool,
}

impl RpcAuthState {
    /// Return an authenticated request marker.
    #[must_use]
    pub(crate) const fn authenticated() -> Self {
        Self {
            authenticated: true,
        }
    }

    /// Return whether the transport authenticated this request.
    #[must_use]
    pub(crate) const fn is_authenticated(self) -> bool {
        self.authenticated
    }
}

/// Verify an HTTP Basic Authorization header against the configured credentials.
#[must_use]
pub(crate) fn verify_basic_auth_header(header: Option<&str>, user: &str, password: &str) -> bool {
    let Some(header) = header else {
        return false;
    };
    let Some((scheme, encoded)) = header.trim().split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    let Ok(decoded) = BASE64_STANDARD.decode(encoded.trim()) else {
        return false;
    };
    let expected = format!("{user}:{password}");
    decoded.as_slice().ct_eq(expected.as_bytes()).into()
}
