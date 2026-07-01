//! Fast-sync package cache, download, and checksum validation.

use anyhow::Context;
use futures::StreamExt;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

use super::FastSyncPackage;

const FAST_SYNC_DOWNLOAD_ATTEMPTS: usize = 3;

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
    download_package_with_retries(
        url,
        destination,
        FAST_SYNC_DOWNLOAD_ATTEMPTS,
        |url, destination| async move { download_package_once(&url, &destination).await },
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

async fn download_package_once(url: &str, destination: &Path) -> anyhow::Result<()> {
    let response = reqwest::get(url)
        .await
        .with_context(|| format!("downloading fast-sync package {url}"))?
        .error_for_status()
        .with_context(|| format!("fast-sync package download returned an error: {url}"))?;
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
        downloaded_bytes += chunk.len() as u64;
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
mod tests {
    use super::*;

    #[test]
    fn failed_fast_sync_download_cleanup_removes_partial_without_touching_cached_zip() {
        let temp = tempfile::tempdir().expect("temp");
        let partial = temp.path().join("chain.0.acc.zip.part");
        let cached = temp.path().join("chain.0.acc.zip");
        std::fs::write(&partial, b"incomplete package").expect("partial");
        std::fs::write(&cached, b"previous cached package").expect("cached");

        remove_partial_download(&partial).expect("cleanup partial");

        assert!(!partial.exists(), "partial download must be removed");
        assert_eq!(
            std::fs::read(&cached).expect("cached package remains"),
            b"previous cached package"
        );
    }

    #[tokio::test]
    async fn fast_sync_download_retries_transient_failure_with_clean_partial_file() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let temp = tempfile::tempdir().expect("temp");
        let partial = temp.path().join("chain.0.acc.zip.part");
        let attempts = Arc::new(AtomicUsize::new(0));

        download_package_with_retries("https://example.invalid/chain.0.acc.zip", &partial, 2, {
            let attempts = Arc::clone(&attempts);
            move |_url, destination| {
                let attempts = Arc::clone(&attempts);
                async move {
                    let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                    match attempt {
                        1 => {
                            std::fs::write(&destination, b"incomplete package")
                                .expect("partial write");
                            anyhow::bail!("simulated response EOF")
                        }
                        2 => {
                            assert!(
                                !destination.exists(),
                                "partial file from the failed attempt must be removed before retry"
                            );
                            std::fs::write(&destination, b"complete package")
                                .expect("complete write");
                            Ok(())
                        }
                        other => panic!("unexpected download attempt {other}"),
                    }
                }
            }
        })
        .await
        .expect("second download attempt should succeed");

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(
            std::fs::read(&partial).expect("downloaded package"),
            b"complete package"
        );
    }

    #[test]
    fn downloaded_content_length_mismatch_is_an_error() {
        let err =
            validate_downloaded_content_length("https://example.invalid/chain.zip", Some(12), 8)
                .expect_err("short fast-sync package download must fail");

        assert!(
            err.to_string().contains("content length mismatch"),
            "unexpected error: {err}"
        );
        assert!(
            err.to_string()
                .contains("expected 12 bytes, downloaded 8 bytes"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parses_md5sum_digest_output() {
        let digest = parse_md5_digest_output(
            "md5sum",
            Path::new("chain.0.acc.zip"),
            b"abcdef0123456789abcdef0123456789  chain.0.acc.zip\n",
        )
        .expect("parse md5sum output");

        assert_eq!(digest, "ABCDEF0123456789ABCDEF0123456789");
    }

    #[test]
    fn parses_bsd_md5_digest_output() {
        let digest = parse_md5_digest_output(
            "md5",
            Path::new("chain.0.acc.zip"),
            b"abcdef0123456789abcdef0123456789\n",
        )
        .expect("parse md5 output");

        assert_eq!(digest, "ABCDEF0123456789ABCDEF0123456789");
    }
}
