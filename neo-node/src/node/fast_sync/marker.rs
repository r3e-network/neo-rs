//! Crash-safety marker for fast-sync package imports.
//!
//! Fast-sync imports intentionally write large local-ledger batches before the
//! node starts live sync. This marker records an in-progress package import so
//! a restarted node refuses to continue on a possibly partial local ledger until
//! the operator restores a checkpoint or removes the local data deliberately.

use std::path::{Path, PathBuf};

use anyhow::Context;

use super::package::FastSyncPackage;

const FAST_SYNC_IMPORT_IN_PROGRESS_MARKER: &str = ".neo-fast-sync-import-in-progress";

fn fast_sync_import_marker_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join(FAST_SYNC_IMPORT_IN_PROGRESS_MARKER)
}

pub(super) fn refuse_stale_fast_sync_import_marker(cache_dir: &Path) -> anyhow::Result<()> {
    let marker_path = fast_sync_import_marker_path(cache_dir);
    if marker_path.exists() {
        anyhow::bail!(
            "previous fast-sync import did not finish cleanly (marker: {}); restore a checkpoint or remove the local ledger before retrying, then remove this marker",
            marker_path.display()
        );
    }
    Ok(())
}

pub(super) fn write_fast_sync_import_marker(
    cache_dir: &Path,
    package: &FastSyncPackage,
    chain_path: &Path,
) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(cache_dir)
        .with_context(|| format!("creating fast-sync cache {}", cache_dir.display()))?;
    let marker_path = fast_sync_import_marker_path(cache_dir);
    let content = format!(
        "network={}\nstart={}\nend={}\npackage={}\nchain={}\n",
        package.network_key,
        package.start,
        package.end,
        package.filename,
        chain_path.display()
    );
    std::fs::write(&marker_path, content)
        .with_context(|| format!("writing fast-sync import marker {}", marker_path.display()))?;
    Ok(marker_path)
}

pub(super) fn clear_fast_sync_import_marker(marker_path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(marker_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err)
            .with_context(|| format!("removing fast-sync import marker {}", marker_path.display())),
    }
}
