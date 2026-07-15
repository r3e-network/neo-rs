//! Official fast-sync package acquisition facade.

use anyhow::Context;
use std::time::Duration;

mod cache;
mod extract;
mod manifest;

pub(super) use cache::ensure_package_cached;
pub(super) use extract::ensure_chain_acc_extracted;
pub(super) use manifest::fetch_latest_package;

const FAST_SYNC_CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const FAST_SYNC_READ_TIMEOUT: Duration = Duration::from_secs(60);

fn secure_http_client() -> anyhow::Result<reqwest::Client> {
    reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(FAST_SYNC_CONNECT_TIMEOUT)
        .read_timeout(FAST_SYNC_READ_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("building HTTPS-only fast-sync client")
}

fn ensure_https_url(url: &url::Url, description: &str) -> anyhow::Result<()> {
    if url.scheme() != "https" {
        anyhow::bail!("{description} resolved to non-HTTPS URL {url}");
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FastSyncPackage {
    pub(super) network_key: &'static str,
    pub(super) url: String,
    /// Integrity digest from the official NGD manifest (MD5). Not treated as
    /// cryptographic authenticity by itself; see [`Self::sha256`].
    pub(super) md5: String,
    /// Optional strong content hash. When present, package acceptance requires
    /// a SHA-256 match before promotion into the cache (auth-grade check).
    pub(super) sha256: Option<String>,
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) filename: String,
}
