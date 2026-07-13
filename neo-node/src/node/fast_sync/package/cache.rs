//! Fast-sync package cache, download, and checksum validation.

use anyhow::Context;
use futures::StreamExt;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use sysinfo::{DiskExt, System, SystemExt};
use tracing::{info, warn};

use super::{FastSyncPackage, ensure_https_url, secure_http_client};

const FAST_SYNC_DOWNLOAD_ATTEMPTS: usize = 3;
const DEFAULT_MAX_FAST_SYNC_PACKAGE_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const MAX_PACKAGE_BYTES_ENV: &str = "NEO_FAST_SYNC_MAX_PACKAGE_BYTES";

pub(in crate::node::fast_sync) async fn ensure_package_cached(
    package: &FastSyncPackage,
    cache_dir: &Path,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating fast-sync cache {}", cache_dir.display()))?;
    let zip_path = cache_dir.join(&package.filename);
    if package_is_valid(&zip_path, &package.md5) {
        info!(
            target: "neo::fast_sync",
            package = %zip_path.display(),
            "using cached fast-sync package"
        );
        return Ok(zip_path);
    }

    if zip_path.exists() {
        warn!(
            target: "neo::fast_sync",
            package = %zip_path.display(),
            "cached fast-sync package failed MD5 validation; downloading again"
        );
    }

    let partial_path = zip_path.with_extension("zip.part");
    let result = download_package(&package.url, &partial_path)
        .await
        .and_then(|()| validate_md5(&partial_path, &package.md5))
        .and_then(|()| replace_cached_package(&partial_path, &zip_path));
    if result.is_err() {
        remove_partial_download(&partial_path)?;
    }
    result?;
    Ok(zip_path)
}

fn replace_cached_package(partial_path: &Path, zip_path: &Path) -> anyhow::Result<()> {
    std::fs::rename(partial_path, zip_path).with_context(|| {
        format!(
            "moving downloaded fast-sync package {} to {}",
            partial_path.display(),
            zip_path.display()
        )
    })
}

fn remove_partial_download(partial_path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(partial_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "removing incomplete fast-sync package {}",
                partial_path.display()
            )
        }),
    }
}

async fn download_package(url: &str, destination: &Path) -> anyhow::Result<()> {
    let client = secure_http_client()?;
    let max_bytes = configured_package_byte_limit()?;
    download_package_with_retries(
        url,
        destination,
        FAST_SYNC_DOWNLOAD_ATTEMPTS,
        move |url, destination| {
            let client = client.clone();
            async move { download_package_once(&client, &url, &destination, max_bytes).await }
        },
    )
    .await
}

async fn download_package_with_retries<F, Fut>(
    url: &str,
    destination: &Path,
    attempts: usize,
    mut download_once: F,
) -> anyhow::Result<()>
where
    F: FnMut(String, PathBuf) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let attempts = attempts.max(1);
    let mut last_error = None;

    for attempt in 1..=attempts {
        match download_once(url.to_string(), destination.to_path_buf()).await {
            Ok(()) => return Ok(()),
            Err(err) => {
                remove_partial_download(destination)?;
                if attempt == attempts {
                    last_error = Some(err);
                    break;
                }
                warn!(
                    target: "neo::fast_sync",
                    url,
                    destination = %destination.display(),
                    attempt,
                    attempts,
                    error = %err,
                    "fast-sync package download attempt failed; retrying"
                );
                last_error = Some(err);
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("fast-sync package download failed: {url}")))
        .with_context(|| {
            format!("fast-sync package download failed after {attempts} attempt(s): {url}")
        })
}

async fn download_package_once(
    client: &reqwest::Client,
    url: &str,
    destination: &Path,
    configured_max_bytes: u64,
) -> anyhow::Result<()> {
    let response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("downloading fast-sync package {url}"))?
        .error_for_status()
        .with_context(|| format!("fast-sync package download returned an error: {url}"))?;
    ensure_https_url(response.url(), "fast-sync package")?;
    let available_bytes = available_space_for(destination)?;
    let byte_limit = configured_max_bytes.min(available_bytes);
    validate_download_limits(url, response.content_length(), byte_limit)?;
    let mut file = std::fs::File::create(destination).with_context(|| {
        format!(
            "creating downloaded fast-sync package {}",
            destination.display()
        )
    })?;
    let expected_content_length = response.content_length();
    let mut downloaded_bytes = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.with_context(|| format!("reading fast-sync package response body: {url}"))?;
        downloaded_bytes = downloaded_bytes
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| anyhow::anyhow!("fast-sync package size overflow for {url}"))?;
        if downloaded_bytes > byte_limit {
            anyhow::bail!(
                "fast-sync package exceeds the {byte_limit}-byte download limit for {url}"
            );
        }
        file.write_all(&chunk).with_context(|| {
            format!(
                "writing downloaded fast-sync package {}",
                destination.display()
            )
        })?;
    }
    validate_downloaded_content_length(url, expected_content_length, downloaded_bytes)?;
    file.flush().with_context(|| {
        format!(
            "flushing downloaded fast-sync package {}",
            destination.display()
        )
    })?;
    Ok(())
}

fn configured_package_byte_limit() -> anyhow::Result<u64> {
    parse_package_byte_limit(std::env::var_os(MAX_PACKAGE_BYTES_ENV).as_deref())
}

fn parse_package_byte_limit(raw: Option<&std::ffi::OsStr>) -> anyhow::Result<u64> {
    let Some(raw) = raw else {
        return Ok(DEFAULT_MAX_FAST_SYNC_PACKAGE_BYTES);
    };
    let value = raw
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("{MAX_PACKAGE_BYTES_ENV} is not valid UTF-8"))?
        .parse::<u64>()
        .with_context(|| format!("parsing {MAX_PACKAGE_BYTES_ENV} as bytes"))?;
    if value == 0 {
        anyhow::bail!("{MAX_PACKAGE_BYTES_ENV} must be greater than zero");
    }
    Ok(value)
}

fn available_space_for(destination: &Path) -> anyhow::Result<u64> {
    let parent = destination
        .parent()
        .ok_or_else(|| anyhow::anyhow!("fast-sync destination has no parent directory"))?;
    let canonical_parent = parent.canonicalize().with_context(|| {
        format!(
            "resolving fast-sync destination directory {}",
            parent.display()
        )
    })?;
    let mut system = System::new();
    system.refresh_disks_list();
    system.refresh_disks();
    system
        .disks()
        .iter()
        .filter(|disk| canonical_parent.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().components().count())
        .map(DiskExt::available_space)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unable to determine available space for {}",
                canonical_parent.display()
            )
        })
}

fn validate_download_limits(
    url: &str,
    expected_content_length: Option<u64>,
    byte_limit: u64,
) -> anyhow::Result<()> {
    if byte_limit == 0 {
        anyhow::bail!("no disk space is available for fast-sync package {url}");
    }
    if expected_content_length.is_some_and(|length| length > byte_limit) {
        anyhow::bail!(
            "fast-sync package content length exceeds the {byte_limit}-byte download limit for {url}"
        );
    }
    Ok(())
}

fn validate_downloaded_content_length(
    url: &str,
    expected_content_length: Option<u64>,
    downloaded_bytes: u64,
) -> anyhow::Result<()> {
    if let Some(expected_bytes) = expected_content_length {
        if downloaded_bytes != expected_bytes {
            anyhow::bail!(
                "fast-sync package content length mismatch for {url}: expected {expected_bytes} bytes, downloaded {downloaded_bytes} bytes"
            );
        }
    }
    Ok(())
}

fn package_is_valid(path: &Path, expected_md5: &str) -> bool {
    path.is_file() && validate_md5(path, expected_md5).is_ok()
}

fn validate_md5(path: &Path, expected_md5: &str) -> anyhow::Result<()> {
    let actual = read_md5_digest(path)?;
    if actual != expected_md5.to_ascii_uppercase() {
        anyhow::bail!(
            "fast-sync package MD5 mismatch for {}: expected {}, got {}",
            path.display(),
            expected_md5,
            actual
        );
    }
    Ok(())
}

fn read_md5_digest(path: &Path) -> anyhow::Result<String> {
    let md5sum = run_md5_digest_command("md5sum", &[], path);
    match md5sum {
        Ok(digest) => Ok(digest),
        Err(md5sum_err) => run_md5_digest_command("md5", &["-q"], path).map_err(|md5_err| {
            anyhow::anyhow!(
                "unable to validate fast-sync package MD5 for {}: md5sum failed ({md5sum_err}); md5 -q failed ({md5_err})",
                path.display()
            )
        }),
    }
}

fn run_md5_digest_command(command: &str, args: &[&str], path: &Path) -> anyhow::Result<String> {
    let output = Command::new(command)
        .args(args)
        .arg(path)
        .output()
        .with_context(|| format!("running {command} for fast-sync package validation"))?;
    if !output.status.success() {
        anyhow::bail!(
            "{command} failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_md5_digest_output(command, path, &output.stdout)
}

fn parse_md5_digest_output(command: &str, path: &Path, stdout: &[u8]) -> anyhow::Result<String> {
    let stdout = String::from_utf8_lossy(stdout);
    stdout
        .split_whitespace()
        .next()
        .map(|digest| digest.to_ascii_uppercase())
        .ok_or_else(|| anyhow::anyhow!("{command} produced no digest for {}", path.display()))
}

#[cfg(test)]
#[path = "../../../tests/node/fast_sync/package/cache.rs"]
mod tests;
