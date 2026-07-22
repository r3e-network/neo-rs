//! # neo-state-packs store filesystem operations
//!
//! Owns directory durability fences and cleanup of interrupted artifact publications.
//!
//! ## Boundary
//!
//! This module performs only pack-store filesystem maintenance. It does not interpret frame,
//! index, segment, or manifest bytes and does not decide which generation is authoritative.
//!
//! ## Contents
//!
//! - [`clear_stale_temp_files`]: removes only recognized unpublished temporary artifacts.
//! - [`sync_directory`]: durably fences directory entry changes.
//! - [`sync_parent_directory`]: durably fences creation of a store directory.

use super::PackSegmentId;
use super::lease::WRITER_LEASE_FILE;
use super::segment;
use crate::engine::manifest;
use anyhow::{Context, Result, ensure};
use std::fs::{self, File};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default)]
struct InterruptedCreationArtifacts {
    empty_runs_directory: bool,
    pending_initial_segment: bool,
}

/// Validates that an existing root can be used to create a store without
/// overwriting caller-owned data.
///
/// Besides an empty directory, this accepts only the exact artifacts that can
/// survive before initial publication: the stable writer lease, an empty
/// `runs` directory, and the unpublished segment-zero header. A canonical
/// segment-zero file is already a store and belongs to the open path. This
/// read-only pass runs before acquiring the lease so an unrelated non-empty
/// directory is not modified merely by a rejected create call.
pub(super) fn preflight_store_creation(root: &Path) -> Result<()> {
    inspect_interrupted_creation(root).map(|_| ())
}

/// Removes an interrupted initial creation after the caller acquires the
/// writer lease, returning the directory to the state expected by the normal
/// creation path.
pub(super) fn clear_interrupted_store_creation(root: &Path) -> Result<()> {
    let artifacts = inspect_interrupted_creation(root)?;
    let mut changed = false;
    if artifacts.pending_initial_segment {
        let pending = segment::pending_segment_path(root, PackSegmentId::INITIAL);
        fs::remove_file(&pending)
            .with_context(|| format!("remove interrupted initial segment {}", pending.display()))?;
        changed = true;
    }
    if artifacts.empty_runs_directory {
        let runs = root.join("runs");
        fs::remove_dir(&runs).with_context(|| {
            format!("remove interrupted index-run directory {}", runs.display())
        })?;
        changed = true;
    }
    if changed {
        sync_directory(root)?;
    }
    Ok(())
}

fn inspect_interrupted_creation(root: &Path) -> Result<InterruptedCreationArtifacts> {
    let runs = root.join("runs");
    let pending = segment::pending_segment_path(root, PackSegmentId::INITIAL);
    let mut artifacts = InterruptedCreationArtifacts::default();
    for entry in fs::read_dir(root)
        .with_context(|| format!("read pack store directory {}", root.display()))?
    {
        let entry = entry.context("read pack store directory entry")?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("classify pack store entry {}", path.display()))?;
        if entry.file_name() == WRITER_LEASE_FILE {
            ensure!(
                file_type.is_file(),
                "pack store writer lease is not a regular file: {}",
                path.display()
            );
        } else if path == runs {
            ensure!(
                file_type.is_dir(),
                "interrupted index-run path is not a directory: {}",
                path.display()
            );
            ensure!(
                fs::read_dir(&path)
                    .with_context(|| format!(
                        "read interrupted index-run directory {}",
                        path.display()
                    ))?
                    .next()
                    .is_none(),
                "interrupted index-run directory is not empty: {}",
                path.display()
            );
            artifacts.empty_runs_directory = true;
        } else if path == pending {
            ensure!(
                file_type.is_file(),
                "interrupted initial segment is not a regular file: {}",
                path.display()
            );
            artifacts.pending_initial_segment = true;
        } else {
            anyhow::bail!(
                "pack store directory must be empty or contain only interrupted initialization artifacts: {}",
                root.display()
            );
        }
    }
    Ok(artifacts)
}

/// Deletes interrupted segment, run, and manifest publications.
///
/// Only exact names emitted by the pack engine are touched. Segment pending
/// files are recognized only in the store root; similarly named files in the
/// run directory or malformed aliases remain untouched. Callers hold the
/// writer lease so cleanup cannot race a live publication.
pub(super) fn clear_stale_temp_files(root: &Path) -> Result<()> {
    for (directory, contains_segments) in [(root.to_path_buf(), true), (root.join("runs"), false)] {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("read directory {}", directory.display()));
            }
        };
        let mut removed = false;
        for entry in entries {
            let entry = entry.context("read directory entry")?;
            let path = entry.path();
            let is_stale_tmp = path.file_name().is_some_and(|name| {
                (contains_segments && segment::parse_pending_segment_file_name(name).is_some())
                    || name.to_str().is_some_and(|name| {
                        manifest::is_run_temp_file_name(name)
                            || manifest::is_manifest_temp_file_name(name)
                    })
            });
            if is_stale_tmp {
                fs::remove_file(&path)
                    .with_context(|| format!("delete stale temp file {}", path.display()))?;
                removed = true;
            }
        }
        if removed {
            sync_directory(&directory)?;
        }
    }
    Ok(())
}

/// Synchronizes a directory so preceding entry mutations survive a crash.
pub(super) fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("open directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync directory {}", path.display()))
}

/// Synchronizes the directory containing `path` after creating or renaming it.
pub(super) fn sync_parent_directory(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    sync_directory(parent).with_context(|| format!("sync parent of {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::store::PackSegmentId;

    #[test]
    fn cleanup_removes_only_canonical_pending_artifacts_in_their_owned_directories() {
        let root = tempfile::tempdir().expect("tempdir");
        let runs = root.path().join("runs");
        fs::create_dir(&runs).expect("create runs directory");

        let segment_pending = segment::pending_segment_path(root.path(), PackSegmentId::new(1));
        let misplaced_segment_pending =
            runs.join(format!("{}.pending", PackSegmentId::new(2).file_name()));
        let malformed_segment_pending = root.path().join("frames-1.pack.pending");
        let unrelated_pending = root.path().join("operator-data.pending");
        let manifest_temp = root.path().join("manifest-00000000000000000003.tmp");
        let run_temp = runs.join("run-00000000000000000004.idx.tmp");
        for path in [
            &segment_pending,
            &misplaced_segment_pending,
            &malformed_segment_pending,
            &unrelated_pending,
            &manifest_temp,
            &run_temp,
        ] {
            fs::write(path, b"temporary").expect("write temporary artifact");
        }

        clear_stale_temp_files(root.path()).expect("clear stale artifacts");

        assert!(!segment_pending.exists());
        assert!(!manifest_temp.exists());
        assert!(!run_temp.exists());
        assert!(misplaced_segment_pending.exists());
        assert!(malformed_segment_pending.exists());
        assert!(unrelated_pending.exists());
    }

    #[test]
    fn creation_cleanup_accepts_only_exact_empty_initialization_artifacts() {
        let root = tempfile::tempdir().expect("tempdir");
        let runs = root.path().join("runs");
        fs::create_dir(&runs).expect("create runs directory");
        let pending = segment::pending_segment_path(root.path(), PackSegmentId::INITIAL);
        fs::write(&pending, b"partial header").expect("write pending segment");
        fs::write(root.path().join(WRITER_LEASE_FILE), b"").expect("write lease");

        preflight_store_creation(root.path()).expect("recognize interrupted creation");
        clear_interrupted_store_creation(root.path()).expect("clear interrupted creation");
        assert!(!runs.exists());
        assert!(!pending.exists());
        assert!(root.path().join(WRITER_LEASE_FILE).is_file());

        fs::write(root.path().join("operator-data"), b"keep").expect("write unrelated data");
        assert!(preflight_store_creation(root.path()).is_err());
        assert!(clear_interrupted_store_creation(root.path()).is_err());
        assert_eq!(
            fs::read(root.path().join("operator-data")).expect("read unrelated data"),
            b"keep"
        );
    }
}
