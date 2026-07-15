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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DownloadPlan {
    append: bool,
    starting_bytes: u64,
    expected_total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentRange {
    start: u64,
    end: u64,
    total: u64,
}

pub(in crate::node::fast_sync) async fn ensure_package_cached(
    package: &FastSyncPackage,
    cache_dir: &Path,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating fast-sync cache {}", cache_dir.display()))?;
    let zip_path = cache_dir.join(&package.filename);
    if package_is_valid(&zip_path, package) {
        info!(
            target: "neo::fast_sync",
            package = %zip_path.display(),
            has_sha256 = package.sha256.is_some(),
            "using cached fast-sync package"
        );
        return Ok(zip_path);
    }

    if zip_path.exists() {
        warn!(
            target: "neo::fast_sync",
            package = %zip_path.display(),
            "cached fast-sync package failed integrity validation; downloading again"
        );
    }

    let partial_path = zip_path.with_extension("zip.part");
    if let Err(error) = download_package(&package.url, &partial_path).await {
        warn!(
            target: "neo::fast_sync",
            package = %partial_path.display(),
            preserved_bytes = partial_download_len(&partial_path),
            error = %error,
            "preserving partial fast-sync package for a later resume"
        );
        return Err(error);
    }
    if let Err(error) = validate_package_digests(&partial_path, package) {
        // Fail closed: corrupt or wrong-hash packages never promote into cache.
        remove_partial_download(&partial_path)?;
        return Err(error);
    }
    replace_cached_package(&partial_path, &zip_path)?;
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
                if attempt == attempts {
                    last_error = Some(err);
                    break;
                }
                warn!(
                    target: "neo::fast_sync",
                    url,
                    destination = %destination.display(),
                    resume_bytes = partial_download_len(destination),
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
    let requested_offset = partial_download_len(destination);
    let mut request = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity");
    if requested_offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={requested_offset}-"));
    }
    let response = request
        .send()
        .await
        .with_context(|| format!("downloading fast-sync package {url}"))?;
    ensure_https_url(response.url(), "fast-sync package")?;
    if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        let total = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_unsatisfied_content_range);
        if total == Some(requested_offset) {
            return Ok(());
        }
        remove_partial_download(destination)?;
        anyhow::bail!(
            "fast-sync server rejected resume offset {requested_offset} for {url}; discarded incompatible partial package"
        );
    }
    let response = response
        .error_for_status()
        .with_context(|| format!("fast-sync package download returned an error: {url}"))?;
    let content_length = response.content_length();
    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .map(|value| value.to_str())
        .transpose()
        .context("fast-sync package Content-Range is not valid ASCII")?;
    let plan = match download_plan(
        response.status(),
        requested_offset,
        content_length,
        content_range,
    ) {
        Ok(plan) => plan,
        Err(error) => {
            if requested_offset > 0 {
                remove_partial_download(destination)?;
            }
            return Err(error).context("validating fast-sync resume response");
        }
    };
    if requested_offset > 0 && !plan.append {
        warn!(
            target: "neo::fast_sync",
            url,
            requested_offset,
            "fast-sync server ignored the byte-range request; restarting this response from zero"
        );
    } else if plan.append {
        info!(
            target: "neo::fast_sync",
            url,
            resumed_from_bytes = plan.starting_bytes,
            expected_total_bytes = plan.expected_total_bytes,
            "resuming partial fast-sync package download"
        );
    }
    let available_bytes = available_space_for(destination)?;
    let byte_limit = configured_max_bytes.min(available_bytes.saturating_add(requested_offset));
    validate_download_limits(url, plan.expected_total_bytes, byte_limit)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    if plan.append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    let mut file = options.open(destination).with_context(|| {
        format!(
            "opening downloaded fast-sync package {}",
            destination.display()
        )
    })?;
    let mut downloaded_bytes = plan.starting_bytes;
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
    validate_downloaded_content_length(url, plan.expected_total_bytes, downloaded_bytes)?;
    file.flush().with_context(|| {
        format!(
            "flushing downloaded fast-sync package {}",
            destination.display()
        )
    })?;
    Ok(())
}

fn partial_download_len(path: &Path) -> u64 {
    path.metadata().map(|metadata| metadata.len()).unwrap_or(0)
}

fn download_plan(
    status: reqwest::StatusCode,
    requested_offset: u64,
    content_length: Option<u64>,
    content_range: Option<&str>,
) -> anyhow::Result<DownloadPlan> {
    if status == reqwest::StatusCode::OK {
        return Ok(DownloadPlan {
            append: false,
            starting_bytes: 0,
            expected_total_bytes: content_length,
        });
    }
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        anyhow::bail!("unexpected successful fast-sync HTTP status {status}");
    }

    let range = content_range
        .ok_or_else(|| anyhow::anyhow!("partial fast-sync response is missing Content-Range"))
        .and_then(parse_content_range)?;
    if range.start != requested_offset {
        anyhow::bail!(
            "fast-sync Content-Range starts at {}, requested offset was {}",
            range.start,
            requested_offset
        );
    }
    let range_bytes = range
        .end
        .checked_sub(range.start)
        .and_then(|length| length.checked_add(1))
        .ok_or_else(|| anyhow::anyhow!("invalid fast-sync Content-Range length"))?;
    if content_length.is_some_and(|length| length != range_bytes) {
        anyhow::bail!(
            "fast-sync Content-Length does not match Content-Range: content_length={:?}, range_bytes={range_bytes}",
            content_length
        );
    }
    Ok(DownloadPlan {
        append: requested_offset > 0,
        starting_bytes: requested_offset,
        expected_total_bytes: Some(range.total),
    })
}

fn parse_content_range(raw: &str) -> anyhow::Result<ContentRange> {
    let value = raw
        .strip_prefix("bytes ")
        .ok_or_else(|| anyhow::anyhow!("invalid fast-sync Content-Range unit: {raw}"))?;
    let (range, total) = value
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("invalid fast-sync Content-Range: {raw}"))?;
    let (start, end) = range
        .split_once('-')
        .ok_or_else(|| anyhow::anyhow!("invalid fast-sync Content-Range: {raw}"))?;
    let parsed = ContentRange {
        start: start.parse().context("parsing Content-Range start")?,
        end: end.parse().context("parsing Content-Range end")?,
        total: total.parse().context("parsing Content-Range total")?,
    };
    if parsed.start > parsed.end || parsed.end >= parsed.total {
        anyhow::bail!("invalid fast-sync Content-Range bounds: {raw}");
    }
    Ok(parsed)
}

fn parse_unsatisfied_content_range(raw: &str) -> Option<u64> {
    raw.strip_prefix("bytes */")?.parse().ok()
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

fn package_is_valid(path: &Path, package: &FastSyncPackage) -> bool {
    path.is_file() && validate_package_digests(path, package).is_ok()
}

/// Validates package bytes before cache promotion.
///
/// Order is intentional:
/// 1. When a SHA-256 digest is present, it is required (auth-grade content hash).
/// 2. MD5 is always checked for NGD manifest compatibility / integrity.
///
/// Either mismatch fails closed; callers must discard the partial download.
fn validate_package_digests(path: &Path, package: &FastSyncPackage) -> anyhow::Result<()> {
    if let Some(expected_sha256) = package.sha256.as_deref() {
        validate_sha256(path, expected_sha256)?;
    }
    validate_md5(path, &package.md5)
}

fn validate_md5(path: &Path, expected_md5: &str) -> anyhow::Result<()> {
    let actual = read_digest(path, DigestKind::Md5)?;
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

fn validate_sha256(path: &Path, expected_sha256: &str) -> anyhow::Result<()> {
    let actual = read_digest(path, DigestKind::Sha256)?;
    if actual != expected_sha256.to_ascii_uppercase() {
        anyhow::bail!(
            "fast-sync package SHA-256 mismatch for {}: expected {}, got {}",
            path.display(),
            expected_sha256,
            actual
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum DigestKind {
    Md5,
    Sha256,
}

impl DigestKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Sha256 => "SHA-256",
        }
    }

    const fn primary_command(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Md5 => ("md5sum", &[]),
            Self::Sha256 => ("sha256sum", &[]),
        }
    }

    const fn fallback_command(self) -> Option<(&'static str, &'static [&'static str])> {
        match self {
            Self::Md5 => Some(("md5", &["-q"])),
            // OpenSSL is widely available when `sha256sum` is missing.
            Self::Sha256 => Some(("openssl", &["dgst", "-sha256"])),
        }
    }
}

fn read_digest(path: &Path, kind: DigestKind) -> anyhow::Result<String> {
    let (primary, primary_args) = kind.primary_command();
    match run_digest_command(primary, primary_args, path, kind) {
        Ok(digest) => Ok(digest),
        Err(primary_err) => {
            let Some((fallback, fallback_args)) = kind.fallback_command() else {
                return Err(primary_err);
            };
            run_digest_command(fallback, fallback_args, path, kind).map_err(|fallback_err| {
                anyhow::anyhow!(
                    "unable to validate fast-sync package {} for {}: {primary} failed ({primary_err}); {fallback} failed ({fallback_err})",
                    kind.label(),
                    path.display()
                )
            })
        }
    }
}

fn run_digest_command(
    command: &str,
    args: &[&str],
    path: &Path,
    kind: DigestKind,
) -> anyhow::Result<String> {
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
    parse_digest_output(command, path, &output.stdout, kind)
}

fn parse_digest_output(
    command: &str,
    path: &Path,
    stdout: &[u8],
    kind: DigestKind,
) -> anyhow::Result<String> {
    let stdout = String::from_utf8_lossy(stdout);
    // md5sum/sha256sum: "<hex>  <path>"
    // md5 -q: "<hex>"
    // openssl dgst -sha256: "SHA256(<path>)= <hex>" or "SHA2-256(path)= <hex>"
    let token = stdout
        .split_whitespace()
        .find(|part| {
            let len = part.len();
            let want = match kind {
                DigestKind::Md5 => 32,
                DigestKind::Sha256 => 64,
            };
            len == want && part.chars().all(|ch| ch.is_ascii_hexdigit())
        })
        .map(|digest| digest.to_ascii_uppercase())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "{command} produced no {} digest for {}",
                kind.label(),
                path.display()
            )
        })?;
    Ok(token)
}

/// Test/helper re-export of the legacy MD5-only parser for unit coverage.
#[cfg(test)]
fn parse_md5_digest_output(command: &str, path: &Path, stdout: &[u8]) -> anyhow::Result<String> {
    parse_digest_output(command, path, stdout, DigestKind::Md5)
}

#[cfg(test)]
#[path = "../../../tests/node/fast_sync/package/cache.rs"]
mod tests;
