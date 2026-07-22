//! # neo-state-packs store filesystem operations
//!
//! Owns directory durability fences and cleanup of interrupted derived-file publications.
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

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::path::Path;

/// Deletes leftover temp files from interrupted run or manifest
/// publications. Only `.tmp` artifacts of the pack engine's own naming scheme
/// are touched. Callers hold the writer lease so cleanup cannot race a live
/// publication.
pub(super) fn clear_stale_temp_files(root: &Path) -> Result<()> {
    for directory in [root.to_path_buf(), root.join("runs")] {
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
            let is_stale_tmp =
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.ends_with(".tmp")
                            && (name.starts_with("run-") || name.starts_with("manifest-"))
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
