//! RPC server configuration formatting and defaults.
//!
//! The root settings module owns the serde-visible configuration schema and the
//! process-wide registry. This module keeps derived behavior for that config
//! record separate: redacted debug output and C#-compatible default values.

use super::RpcServerConfig;

impl std::fmt::Debug for RpcServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcServerConfig")
            .field("network", &self.network)
            .field("bind_address", &self.bind_address)
            .field("port", &self.port)
            .field("ssl_cert", &self.ssl_cert)
            .field("ssl_cert_password", &"[redacted]")
            .field("trusted_authorities", &self.trusted_authorities)
            .field(
                "max_concurrent_connections",
                &self.max_concurrent_connections,
            )
            .field("max_requests_per_second", &self.max_requests_per_second)
            .field("rate_limit_burst", &self.rate_limit_burst)
            .field("max_request_body_size", &self.max_request_body_size)
            .field("rpc_user", &self.rpc_user)
            .field("rpc_pass", &"[redacted]")
            .field("enable_cors", &self.enable_cors)
            .field("allow_origins", &self.allow_origins)
            .field("keep_alive_timeout", &self.keep_alive_timeout)
            .field("request_headers_timeout", &self.request_headers_timeout)
            .field("max_gas_invoke", &self.max_gas_invoke)
            .field("max_fee", &self.max_fee)
            .field("max_iterator_result_items", &self.max_iterator_result_items)
            .field("max_stack_size", &self.max_stack_size)
            .field("disabled_methods", &self.disabled_methods)
            .field("session_enabled", &self.session_enabled)
            .field("session_expiration_time", &self.session_expiration_time)
            .field("find_storage_page_size", &self.find_storage_page_size)
            .field("max_batch_size", &self.max_batch_size)
            .finish()
    }
}

impl Default for RpcServerConfig {
    fn default() -> Self {
        Self {
            network: Self::default_network(),
            bind_address: Self::default_bind_address(),
            port: Self::default_port(),
            ssl_cert: String::new(),
            ssl_cert_password: String::new(),
            trusted_authorities: Vec::new(),
            max_concurrent_connections: Self::default_max_concurrent_connections(),
            max_requests_per_second: Self::default_max_requests_per_second(),
            rate_limit_burst: Self::default_rate_limit_burst(),
            max_request_body_size: Self::default_max_request_body_size(),
            rpc_user: String::new(),
            rpc_pass: String::new(),
            enable_cors: Self::default_enable_cors(),
            allow_origins: Vec::new(),
            keep_alive_timeout: Self::default_keep_alive_timeout(),
            request_headers_timeout: Self::default_request_headers_timeout(),
            max_gas_invoke: Self::default_max_gas_invocation(),
            max_fee: Self::default_max_fee(),
            max_iterator_result_items: Self::default_max_iterator_result_items(),
            max_stack_size: Self::default_max_stack_size(),
            disabled_methods: Vec::new(),
            session_enabled: false,
            session_expiration_time: Self::default_session_expiration_seconds(),
            find_storage_page_size: Self::default_find_storage_page_size(),
            max_batch_size: Self::default_max_batch_size(),
        }
    }
}
