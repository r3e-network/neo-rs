//! Official fast-sync manifest parsing and package selection.

use anyhow::Context;
use serde::Deserialize;

use super::FastSyncPackage;

const OFFICIAL_FAST_SYNC_MANIFEST_URL: &str = "https://sync.ngd.network/config.json";
const MAINNET_MAGIC: u32 = 0x334F_454E;
const TESTNET_MAGIC: u32 = 0x3554_334E;

#[derive(Debug, Deserialize)]
struct SyncManifest {
    #[serde(default)]
    n3mainnet: Option<NetworkPackages>,
    #[serde(default)]
    n3testnet: Option<NetworkPackages>,
}

#[derive(Debug, Deserialize)]
struct NetworkPackages {
    full: PackageEntry,
}

#[derive(Debug, Deserialize)]
struct PackageEntry {
    path: String,
    md5: String,
    start: u32,
    end: u32,
}

pub(in crate::node::fast_sync) async fn fetch_latest_package(
    network: u32,
) -> anyhow::Result<FastSyncPackage> {
    let manifest = reqwest::get(OFFICIAL_FAST_SYNC_MANIFEST_URL)
        .await
        .context("requesting official fast-sync manifest")?
        .error_for_status()
        .context("official fast-sync manifest returned an error")?
        .json::<SyncManifest>()
        .await
        .context("decoding official fast-sync manifest")?;
    select_full_package(&manifest, network)
}

fn select_full_package(manifest: &SyncManifest, network: u32) -> anyhow::Result<FastSyncPackage> {
    let (network_key, packages) = match network {
        MAINNET_MAGIC => (
            "n3mainnet",
            manifest
                .n3mainnet
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("manifest is missing n3mainnet package data"))?,
        ),
        TESTNET_MAGIC => (
            "n3testnet",
            manifest
                .n3testnet
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("manifest is missing n3testnet package data"))?,
        ),
        other => {
            anyhow::bail!(
                "built-in fast sync supports Neo N3 MainNet/TestNet only, got network 0x{other:08X}"
            )
        }
    };

    let entry = &packages.full;
    if entry.path.trim().is_empty() {
        anyhow::bail!("manifest {network_key}.full.path is empty");
    }
    if entry.md5.trim().len() != 32 || !entry.md5.chars().all(|ch| ch.is_ascii_hexdigit()) {
        anyhow::bail!("manifest {network_key}.full.md5 is not a valid MD5 hex digest");
    }
    if entry.start > entry.end {
        anyhow::bail!(
            "manifest {network_key}.full start height {} is greater than end height {}",
            entry.start,
            entry.end
        );
    }
    let filename = package_filename(&entry.path)?;
    Ok(FastSyncPackage {
        network_key,
        url: entry.path.clone(),
        md5: entry.md5.to_ascii_uppercase(),
        start: entry.start,
        end: entry.end,
        filename,
    })
}

fn package_filename(url: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(url).context("manifest package URL is invalid")?;
    match parsed.scheme() {
        "http" | "https" => {}
        scheme => anyhow::bail!(
            "manifest package URL uses unsupported URL scheme {scheme:?}; expected http or https"
        ),
    }
    let filename = parsed
        .path_segments()
        .and_then(Iterator::last)
        .filter(|segment| !segment.is_empty())
        .ok_or_else(|| anyhow::anyhow!("manifest package URL has no file name"))?;
    if !filename.ends_with(".zip") {
        anyhow::bail!("manifest package URL must point to a .zip file");
    }
    Ok(filename.to_string())
}

#[cfg(test)]
#[path = "../../../tests/node/fast_sync/package/manifest.rs"]
mod tests;
