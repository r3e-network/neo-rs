//! Fast-sync package extraction and extracted `chain.acc` cache validation.

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::process::Command;

const FAST_SYNC_EXTRACT_MD5_MARKER: &str = ".neo-fast-sync-package-md5";

pub(in crate::node::fast_sync) fn ensure_chain_acc_extracted(
    zip_path: &Path,
    cache_dir: &Path,
    package_md5: &str,
) -> anyhow::Result<PathBuf> {
    let stem = zip_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid fast-sync package name {}", zip_path.display()))?;
    let extract_dir = cache_dir.join(stem);
    if let Some(chain_path) = cached_extracted_chain_acc(&extract_dir, package_md5)? {
        return Ok(chain_path);
    }
    ensure_command_available("unzip")?;
    if extract_dir.exists() {
        std::fs::remove_dir_all(&extract_dir).with_context(|| {
            format!(
                "removing stale fast-sync extract directory {}",
                extract_dir.display()
            )
        })?;
    }
    std::fs::create_dir_all(&extract_dir).with_context(|| {
        format!(
            "creating fast-sync extract directory {}",
            extract_dir.display()
        )
    })?;
    let status = Command::new("unzip")
        .arg("-o")
        .arg(zip_path)
        .arg("-d")
        .arg(&extract_dir)
        .status()
        .with_context(|| "running unzip for fast-sync package extraction")?;
    if !status.success() {
        remove_partial_extract_dir(&extract_dir)?;
        anyhow::bail!("unzip failed for fast-sync package {}", zip_path.display());
    }
    let chain_path = find_extracted_chain_acc(&extract_dir)?;
    write_extract_md5_marker(&extract_dir, package_md5, &chain_path)?;
    Ok(chain_path)
}

fn ensure_command_available(command: &str) -> anyhow::Result<()> {
    let status = Command::new(command)
        .arg("-v")
        .output()
        .with_context(|| {
            format!(
                "required command `{command}` is not available; install it or use a fast-sync package cache that is already extracted"
            )
        })?;
    if !status.status.success() {
        anyhow::bail!(
            "required command `{command}` is not available; install it or use a fast-sync package cache that is already extracted"
        );
    }
    Ok(())
}

fn remove_partial_extract_dir(extract_dir: &Path) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(extract_dir) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err).with_context(|| {
            format!(
                "removing incomplete fast-sync extract directory {}",
                extract_dir.display()
            )
        }),
    }
}

fn cached_extracted_chain_acc(
    extract_dir: &Path,
    package_md5: &str,
) -> anyhow::Result<Option<PathBuf>> {
    if !extract_dir.is_dir() {
        return Ok(None);
    }
    let marker_path = extract_dir.join(FAST_SYNC_EXTRACT_MD5_MARKER);
    let Ok(marker) = std::fs::read_to_string(&marker_path) else {
        return Ok(None);
    };
    let chain_path = find_extracted_chain_acc(extract_dir)?;
    if !chain_acc_file_is_non_empty(&chain_path)? {
        return Ok(None);
    }
    if !extract_marker_matches_chain_acc(&marker, package_md5, &chain_path)? {
        return Ok(None);
    }
    Ok(Some(chain_path))
}

fn extract_marker_matches_chain_acc(
    marker: &str,
    package_md5: &str,
    chain_path: &Path,
) -> anyhow::Result<bool> {
    let Some(marker_md5) = read_extract_marker_value(marker, "package_md5") else {
        return Ok(false);
    };
    if !marker_md5.eq_ignore_ascii_case(package_md5.trim()) {
        return Ok(false);
    }
    let Some(marker_chain_bytes) = read_extract_marker_value(marker, "chain_bytes") else {
        return Ok(false);
    };
    let Ok(marker_chain_bytes) = marker_chain_bytes.parse::<u64>() else {
        return Ok(false);
    };
    let actual_chain_bytes = std::fs::metadata(chain_path)
        .with_context(|| format!("reading metadata for {}", chain_path.display()))?
        .len();
    Ok(marker_chain_bytes == actual_chain_bytes)
}

fn read_extract_marker_value<'a>(marker: &'a str, key: &str) -> Option<&'a str> {
    marker.lines().find_map(|line| {
        let (line_key, value) = line.split_once('=')?;
        (line_key.trim() == key).then(|| value.trim())
    })
}

fn write_extract_md5_marker(
    extract_dir: &Path,
    package_md5: &str,
    chain_path: &Path,
) -> anyhow::Result<()> {
    let chain_bytes = std::fs::metadata(chain_path)
        .with_context(|| format!("reading metadata for {}", chain_path.display()))?
        .len();
    std::fs::write(
        extract_dir.join(FAST_SYNC_EXTRACT_MD5_MARKER),
        format!(
            "package_md5={}\nchain_bytes={chain_bytes}\n",
            package_md5.to_ascii_uppercase()
        ),
    )
    .with_context(|| {
        format!(
            "writing fast-sync extract marker under {}",
            extract_dir.display()
        )
    })
}

fn find_extracted_chain_acc(extract_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut candidates = Vec::new();
    collect_chain_acc_files(extract_dir, &mut candidates)?;
    candidates.sort();
    match candidates.len() {
        1 => Ok(candidates.remove(0)),
        0 => anyhow::bail!(
            "fast-sync package extraction produced no chain*.acc file under {}",
            extract_dir.display()
        ),
        _ => anyhow::bail!(
            "fast-sync package extraction produced multiple chain*.acc files under {}",
            extract_dir.display()
        ),
    }
}

fn chain_acc_file_is_non_empty(path: &Path) -> anyhow::Result<bool> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    Ok(metadata.is_file() && metadata.len() > 0)
}

fn collect_chain_acc_files(dir: &Path, candidates: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_chain_acc_files(&path, candidates)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("chain.") && name.ends_with(".acc"))
        {
            candidates.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/node/fast_sync/package/extract.rs"]
mod tests;
