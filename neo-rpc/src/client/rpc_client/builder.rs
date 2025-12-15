// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_client/builder.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::hooks::RpcClientHooks;
use super::{RpcClient, DEFAULT_HTTP_TIMEOUT};
use base64::{engine::general_purpose, Engine as _};
use neo_config::ProtocolSettings;
use reqwest::{Client, Url};
use std::sync::Arc;
use std::time::Duration;
use zeroize::Zeroizing;

/// Builder for configuring an [`RpcClient`] with timeouts and hooks.
///
/// # Security Note
/// Credentials are stored using [`Zeroizing`] to ensure they are securely
/// cleared from memory when the builder is dropped or after `build()` completes.
pub struct RpcClientBuilder {
    base_address: Url,
    rpc_user: Option<Zeroizing<String>>,
    rpc_pass: Option<Zeroizing<String>>,
    protocol_settings: Option<ProtocolSettings>,
    timeout: Duration,
    hooks: RpcClientHooks,
}

impl RpcClientBuilder {
    pub fn new(base_address: Url) -> Self {
        Self {
            base_address,
            rpc_user: None,
            rpc_pass: None,
            protocol_settings: None,
            timeout: DEFAULT_HTTP_TIMEOUT,
            hooks: RpcClientHooks::default(),
        }
    }

    /// Applies basic-auth credentials.
    ///
    /// # Security Note
    /// Credentials are wrapped in [`Zeroizing`] to ensure secure memory handling.
    pub fn with_basic_auth(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.rpc_user = Some(Zeroizing::new(user.into()));
        self.rpc_pass = Some(Zeroizing::new(pass.into()));
        self
    }

    /// Applies optional basic-auth credentials (helper for matching legacy constructor).
    pub fn with_optional_auth(mut self, user: Option<String>, pass: Option<String>) -> Self {
        self.rpc_user = user.map(Zeroizing::new);
        self.rpc_pass = pass.map(Zeroizing::new);
        self
    }

    /// Overrides the protocol settings used for serialisation.
    pub fn protocol_settings(mut self, settings: ProtocolSettings) -> Self {
        self.protocol_settings = Some(settings);
        self
    }

    /// Configures the HTTP client timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Registers hooks for logging/metrics.
    pub fn hooks(mut self, hooks: RpcClientHooks) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn build(self) -> Result<RpcClient, Box<dyn std::error::Error>> {
        let mut client_builder = Client::builder().timeout(self.timeout);

        if let (Some(user), Some(pass)) = (self.rpc_user, self.rpc_pass) {
            // Credentials are in Zeroizing<String>, they will be securely cleared on drop
            let auth = Zeroizing::new(format!("{}:{}", user.as_str(), pass.as_str()));
            let encoded = general_purpose::STANDARD.encode(auth.as_bytes());
            client_builder = client_builder.default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {}", encoded).parse()?,
                );
                headers
            });
            // user, pass, and auth are dropped here, triggering Zeroizing cleanup
        }

        let http_client = client_builder.build()?;

        Ok(RpcClient {
            base_address: self.base_address,
            http_client,
            protocol_settings: Arc::new(self.protocol_settings.unwrap_or_default()),
            request_timeout: self.timeout,
            hooks: self.hooks,
        })
    }
}
