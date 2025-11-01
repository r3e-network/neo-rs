// Copyright (C) 2015-2025 The Neo Project.
//
// Rust helper that mirrors the behaviour of
// `Neo.Plugins.RestServer.Authentication.BasicAuthenticationHandler`.

use crate::rest_server::rest_server_settings::RestServerSettings;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

/// Validates HTTP Basic authentication headers against the configured REST credentials.
pub struct BasicAuthenticationHandler;

impl BasicAuthenticationHandler {
    /// Returns `true` when the provided `Authorization` header satisfies the credentials.
    pub fn authenticate(auth_header: Option<&str>) -> bool {
        let settings = RestServerSettings::current();
        if !settings.enable_basic_authentication {
            return true;
        }

        let header = match auth_header {
            Some(value) => value.trim(),
            None => return false,
        };

        let Some((scheme, param)) = header.split_once(' ') else {
            return false;
        };

        if !scheme.eq_ignore_ascii_case("basic") {
            return false;
        }

        let Ok(decoded) = BASE64.decode(param) else {
            return false;
        };

        let decoded = match std::str::from_utf8(&decoded) {
            Ok(text) => text,
            Err(_) => return false,
        };

        let mut parts = decoded.splitn(2, ':');
        let username = parts.next().unwrap_or_default();
        let password = parts.next().unwrap_or_default();

        subtle_equals(username, &settings.rest_user) && subtle_equals(password, &settings.rest_pass)
    }
}

fn subtle_equals(left: &str, right: &str) -> bool {
    // Constant-time comparison to avoid leaking credential length differences.
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in left.as_bytes().iter().zip(right.as_bytes()) {
        diff |= a ^ b;
    }
    diff == 0
}
