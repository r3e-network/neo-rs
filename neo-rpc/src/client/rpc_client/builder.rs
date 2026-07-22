use super::hooks::{RpcClientHooks, RpcObserver, TracingRpcObserver};
use super::{DEFAULT_HTTP_TIMEOUT, RpcClient};
use crate::client::RpcClientError;
use base64::{Engine as _, engine::general_purpose};
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
pub struct RpcClientBuilder<O = TracingRpcObserver> {
    base_address: Url,
    rpc_user: Option<Zeroizing<String>>,
    rpc_pass: Option<Zeroizing<String>>,
    protocol_settings: Option<ProtocolSettings>,
    timeout: Duration,
    hooks: RpcClientHooks<O>,
}

impl RpcClientBuilder<TracingRpcObserver> {
    /// Create a builder for the provided RPC endpoint URL.
    #[must_use]
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
}

impl<O> RpcClientBuilder<O>
where
    O: RpcObserver,
{
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
    #[must_use]
    pub fn protocol_settings(mut self, settings: ProtocolSettings) -> Self {
        self.protocol_settings = Some(settings);
        self
    }

    /// Configures the HTTP client timeout.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Registers hooks for logging/metrics.
    #[must_use]
    pub fn hooks<T>(self, hooks: RpcClientHooks<T>) -> RpcClientBuilder<T>
    where
        T: RpcObserver,
    {
        let Self {
            base_address,
            rpc_user,
            rpc_pass,
            protocol_settings,
            timeout,
            ..
        } = self;

        RpcClientBuilder {
            base_address,
            rpc_user,
            rpc_pass,
            protocol_settings,
            timeout,
            hooks,
        }
    }

    /// Registers a concrete observer called after each RPC request completes.
    #[must_use]
    pub fn with_observer<T>(self, observer: T) -> RpcClientBuilder<T>
    where
        T: RpcObserver,
    {
        self.hooks(RpcClientHooks::new().with_observer(observer))
    }

    /// Build the configured [`RpcClient`].
    pub fn build(self) -> Result<RpcClient<O>, RpcClientError> {
        let mut client_builder = Client::builder().no_proxy().timeout(self.timeout);

        if let (Some(user), Some(pass)) = (self.rpc_user, self.rpc_pass) {
            // Credentials are in Zeroizing<String>, they will be securely cleared on drop
            let auth = Zeroizing::new(format!("{}:{}", user.as_str(), pass.as_str()));
            let encoded = general_purpose::STANDARD.encode(auth.as_bytes());
            client_builder = client_builder.default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {encoded}").parse()?,
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
