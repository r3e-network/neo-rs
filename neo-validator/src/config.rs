//! Strict configuration and security-boundary validation.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use subtle::ConstantTimeEq;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[derive(Default)]
pub struct ValidatorConfig {
    pub network: NetworkConfig,
    pub validator: ValidationConfig,
    pub neo_cli: UpstreamConfig,
    pub neo_go: UpstreamConfig,
    pub security: SecurityConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    pub magic: Option<u32>,
    pub protocol_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ValidationConfig {
    pub data_path: PathBuf,
    pub audit_path: PathBuf,
    pub poll_interval_ms: u64,
    pub rpc_timeout_ms: u64,
    pub max_catch_up_blocks: u32,
    pub max_block_bytes: usize,
    pub require_application_logs: bool,
    /// `canonical` (default), `auto`, or fail-closed dual-execution `verify`.
    pub execution_mode: String,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            data_path: PathBuf::from("./validator-data"),
            audit_path: PathBuf::from("./validator-audit.jsonl"),
            poll_interval_ms: 500,
            rpc_timeout_ms: 3_000,
            max_catch_up_blocks: 128,
            max_block_bytes: 16 * 1024 * 1024,
            require_application_logs: true,
            execution_mode: "canonical".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UpstreamConfig {
    pub path: PathBuf,
    pub rpc_url: String,
    pub proxy_listen: Option<SocketAddr>,
    pub rpc_username_env: Option<String>,
    pub rpc_password_env: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecurityConfig {
    pub admin_listen: SocketAddr,
    pub admin_token_sha256: String,
    pub read_token_sha256: String,
    pub proxy_token_sha256: Option<String>,
    pub allowed_methods: BTreeSet<String>,
    pub max_request_body_bytes: usize,
    pub max_response_body_bytes: usize,
    pub allow_public_proxy: bool,
    pub allow_remote_upstreams: bool,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            admin_listen: SocketAddr::from(([127, 0, 0, 1], 20400)),
            admin_token_sha256: String::new(),
            read_token_sha256: String::new(),
            proxy_token_sha256: None,
            allowed_methods: BTreeSet::new(),
            max_request_body_bytes: 1024 * 1024,
            max_response_body_bytes: 16 * 1024 * 1024,
            allow_public_proxy: false,
            allow_remote_upstreams: false,
        }
    }
}

#[derive(Clone)]
pub struct TokenDigest([u8; 32]);

impl std::fmt::Debug for TokenDigest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TokenDigest(REDACTED)")
    }
}

impl TokenDigest {
    pub fn parse(value: &str, field: &str) -> Result<Self> {
        let bytes = hex::decode(value).with_context(|| format!("{field} must be hex"))?;
        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("{field} must contain exactly 32 bytes"))?;
        Ok(Self(array))
    }

    pub fn from_token(token: &str) -> Self {
        Self(Sha256::digest(token.as_bytes()).into())
    }

    pub fn verify(&self, presented: &str) -> bool {
        let candidate = Sha256::digest(presented.as_bytes());
        self.0.ct_eq(candidate.as_slice()).into()
    }
}

#[derive(Debug, Clone)]
pub struct UpstreamCredentials {
    pub username: zeroize::Zeroizing<String>,
    pub password: zeroize::Zeroizing<String>,
}

impl UpstreamConfig {
    pub fn credentials(&self) -> Result<Option<UpstreamCredentials>> {
        match (&self.rpc_username_env, &self.rpc_password_env) {
            (None, None) => Ok(None),
            (Some(username_env), Some(password_env)) => {
                for name in [username_env, password_env] {
                    if name.is_empty() || name.contains(['=', '\0']) {
                        bail!("RPC credential environment variable names are invalid");
                    }
                }
                let username = std::env::var(username_env).with_context(|| {
                    format!("RPC username environment variable {username_env} is missing")
                })?;
                let password = std::env::var(password_env).with_context(|| {
                    format!("RPC password environment variable {password_env} is missing")
                })?;
                Ok(Some(UpstreamCredentials {
                    username: zeroize::Zeroizing::new(username),
                    password: zeroize::Zeroizing::new(password),
                }))
            }
            _ => bail!("rpc_username_env and rpc_password_env must be configured together"),
        }
    }

    pub fn parsed_url(&self) -> Result<Url> {
        let url = Url::parse(&self.rpc_url).context("invalid upstream rpc_url")?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            bail!("upstream RPC URL must be absolute HTTP(S)");
        }
        if !url.username().is_empty() || url.password().is_some() {
            bail!("upstream credentials must use environment variables, not rpc_url");
        }
        Ok(url)
    }
}

impl ValidatorConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let config: Self = toml::from_str(&text).context("invalid validator TOML")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        self.validator
            .execution_mode
            .parse::<neo_system::ExecutionMode>()
            .map_err(anyhow::Error::msg)
            .context("invalid validator.execution_mode")?;
        let Some(_) = self.network.magic else {
            bail!("network.magic is required");
        };
        for (name, upstream) in [("neo_cli", &self.neo_cli), ("neo_go", &self.neo_go)] {
            if upstream.path.as_os_str().is_empty() || !upstream.path.is_dir() {
                bail!("{name}.path must name an existing node directory");
            }
            let url = upstream.parsed_url()?;
            if !url_is_loopback(&url) {
                if !self.security.allow_remote_upstreams {
                    bail!("{name} uses a remote upstream without allow_remote_upstreams");
                }
                if url.scheme() != "https" {
                    bail!("remote upstream {name} must use HTTPS");
                }
            }
            let Some(listen) = upstream.proxy_listen else {
                bail!("{name}.proxy_listen is required");
            };
            if !listen.ip().is_loopback() && !self.security.allow_public_proxy {
                bail!("public {name} proxy requires allow_public_proxy");
            }
            if listen == self.security.admin_listen {
                bail!("proxy and admin listeners must be distinct");
            }
            upstream.credentials()?;
        }
        if self.neo_cli.proxy_listen == self.neo_go.proxy_listen {
            bail!("neo_cli and neo_go proxy listeners must be distinct");
        }
        if !self.security.admin_listen.ip().is_loopback() {
            bail!("security.admin_listen must be loopback");
        }
        TokenDigest::parse(&self.security.admin_token_sha256, "admin_token_sha256")?;
        TokenDigest::parse(&self.security.read_token_sha256, "read_token_sha256")?;
        if self.security.admin_token_sha256 == self.security.read_token_sha256 {
            bail!("admin and read tokens must be different");
        }
        if self.security.allow_public_proxy {
            let token = self.security.proxy_token_sha256.as_deref().ok_or_else(|| {
                anyhow::anyhow!("public proxies require security.proxy_token_sha256")
            })?;
            TokenDigest::parse(token, "proxy_token_sha256")?;
        } else if let Some(token) = &self.security.proxy_token_sha256 {
            TokenDigest::parse(token, "proxy_token_sha256")?;
        }
        if self.security.allowed_methods.is_empty()
            || self
                .security
                .allowed_methods
                .iter()
                .any(|method| !valid_method(method))
        {
            bail!("allowed_methods must contain non-empty lowercase RPC method names");
        }
        if !(50..=60_000).contains(&self.validator.poll_interval_ms) {
            bail!("poll_interval_ms must be between 50 and 60000");
        }
        if !(100..=120_000).contains(&self.validator.rpc_timeout_ms) {
            bail!("rpc_timeout_ms must be between 100 and 120000");
        }
        if !(1..=10_000).contains(&self.validator.max_catch_up_blocks) {
            bail!("max_catch_up_blocks must be between 1 and 10000");
        }
        if !self.validator.require_application_logs {
            bail!("strict validation requires require_application_logs = true");
        }
        if self.validator.max_block_bytes < 1_024
            || self.security.max_request_body_bytes < 128
            || self.security.max_response_body_bytes < 128
        {
            bail!("configured byte limits are too small");
        }
        if self.validator.data_path == self.neo_cli.path
            || self.validator.data_path == self.neo_go.path
            || self.validator.data_path.starts_with(&self.neo_cli.path)
            || self.validator.data_path.starts_with(&self.neo_go.path)
        {
            bail!("validator data must be outside both upstream directories");
        }
        if self.validator.audit_path == self.validator.data_path {
            bail!("audit path must not be the validator data directory");
        }
        Ok(())
    }
}

fn valid_method(method: &str) -> bool {
    !method.is_empty()
        && method
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn url_is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        Some(url::Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_uses_digest_not_configuration_plaintext() {
        let digest = TokenDigest::from_token("correct horse battery staple");
        assert!(digest.verify("correct horse battery staple"));
        assert!(!digest.verify("correct horse battery staplf"));
    }

    #[test]
    fn non_loopback_admin_is_rejected() {
        let mut config = ValidatorConfig::default();
        config.security.admin_listen = "0.0.0.0:20400".parse().unwrap();
        assert!(config.validate().is_err());
    }
}
