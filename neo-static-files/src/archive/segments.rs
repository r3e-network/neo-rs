//! Height-addressed archive segment discovery, creation, and routing.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::io::{read_exact_at, sync_parent_directory, write_all_at};
use crate::format::{FILE_HEADER_LEN, file_header, validate_file_header};
use crate::{StaticFileError, StaticFileResult};

const SEGMENT_MARKER: &[u8] = b".segment-";
const SEGMENT_HEIGHT_DIGITS: usize = 10;
const PENDING_SUFFIX: &str = ".pending";

/// One opened immutable-prefix segment.
#[derive(Clone, Debug)]
pub(super) struct ArchiveSegment {
    pub(super) start_height: u32,
    pub(super) path: PathBuf,
    pub(super) file: Arc<File>,
}

impl ArchiveSegment {
    pub(super) fn len(&self) -> StaticFileResult<u64> {
        self.file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|source| StaticFileError::io("read segment metadata", &self.path, source))
    }
}

/// Ordered segment directory shared by readers and the single writer.
#[derive(Debug)]
pub(super) struct ArchiveSegments {
    base_path: PathBuf,
    archive_id: u64,
    segments: BTreeMap<u32, ArchiveSegment>,
}

impl ArchiveSegments {
    pub(super) fn discover(
        base_path: &Path,
        base_file: File,
        archive_id: u64,
    ) -> StaticFileResult<Self> {
        let mut segments = BTreeMap::new();
        segments.insert(
            0,
            ArchiveSegment {
                start_height: 0,
                path: base_path.to_path_buf(),
                file: Arc::new(base_file),
            },
        );

        let parent = parent_directory(base_path);
        let base_name = base_path
            .file_name()
            .ok_or_else(|| StaticFileError::invalid(0, "archive path must have a file name"))?;
        let entries = std::fs::read_dir(parent)
            .map_err(|source| StaticFileError::io("scan segment directory", parent, source))?;
        for entry in entries {
            let entry = entry
                .map_err(|source| StaticFileError::io("read segment entry", parent, source))?;
            let file_name = entry.file_name();
            if is_pending_name(base_name, &file_name) {
                let pending_path = entry.path();
                std::fs::remove_file(&pending_path).map_err(|source| {
                    StaticFileError::io("remove incomplete segment header", &pending_path, source)
                })?;
                sync_parent_directory(&pending_path)?;
                continue;
            }
            let Some(start_height) = parse_segment_start(base_name, &file_name) else {
                if has_segment_marker(base_name, &file_name) {
                    return Err(StaticFileError::invalid(
                        0,
                        format!(
                            "malformed archive segment file name {}",
                            entry.path().display()
                        ),
                    ));
                }
                continue;
            };
            if start_height == 0 {
                return Err(StaticFileError::invalid(
                    0,
                    "rotated segment cannot repeat genesis height",
                ));
            }
            let path = entry.path();
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|source| StaticFileError::io("open archive segment", &path, source))?;
            validate_segment_header(&file, &path, archive_id)?;
            if segments
                .insert(
                    start_height,
                    ArchiveSegment {
                        start_height,
                        path,
                        file: Arc::new(file),
                    },
                )
                .is_some()
            {
                return Err(StaticFileError::invalid(
                    0,
                    format!("duplicate archive segment start height {start_height}"),
                ));
            }
        }

        Ok(Self {
            base_path: base_path.to_path_buf(),
            archive_id,
            segments,
        })
    }

    pub(super) fn snapshots(&self) -> Vec<ArchiveSegment> {
        self.segments.values().cloned().collect()
    }

    pub(super) fn count(&self) -> usize {
        self.segments.len()
    }

    pub(super) fn paths(&self) -> Vec<PathBuf> {
        self.segments
            .values()
            .map(|segment| segment.path.clone())
            .collect()
    }

    pub(super) fn exact(&self, start_height: u32) -> StaticFileResult<ArchiveSegment> {
        self.segments.get(&start_height).cloned().ok_or_else(|| {
            StaticFileError::invalid_index(format!(
                "archive segment beginning at height {start_height} is missing"
            ))
        })
    }

    pub(super) fn create(&mut self, start_height: u32) -> StaticFileResult<ArchiveSegment> {
        if start_height == 0 || self.segments.contains_key(&start_height) {
            return Err(StaticFileError::invalid_index(format!(
                "archive segment start height {start_height} is not new"
            )));
        }
        let path = segment_path(&self.base_path, start_height)?;
        let pending_path = pending_path(&path);
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&pending_path)
            .map_err(|source| {
                StaticFileError::io("create pending archive segment", &pending_path, source)
            })?;
        write_all_at(&file, 0, &file_header(self.archive_id)).map_err(|source| {
            StaticFileError::io("write archive segment header", &pending_path, source)
        })?;
        file.sync_all().map_err(|source| {
            StaticFileError::io("sync archive segment header", &pending_path, source)
        })?;
        drop(file);
        std::fs::rename(&pending_path, &path)
            .map_err(|source| StaticFileError::io("publish archive segment", &path, source))?;
        sync_parent_directory(&path)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|source| {
                StaticFileError::io("open published archive segment", &path, source)
            })?;

        let segment = ArchiveSegment {
            start_height,
            path,
            file: Arc::new(file),
        };
        self.segments.insert(start_height, segment.clone());
        Ok(segment)
    }

    pub(super) fn remove_after(&mut self, start_height: u32) -> StaticFileResult<()> {
        let Some(first_removed) = start_height.checked_add(1) else {
            return Ok(());
        };
        let removed = self.segments.split_off(&first_removed);
        let removed_paths = removed
            .values()
            .map(|segment| segment.path.clone())
            .collect::<Vec<_>>();
        drop(removed);
        for path in removed_paths.iter().rev() {
            std::fs::remove_file(path)
                .map_err(|source| StaticFileError::io("remove archive segment", path, source))?;
            sync_parent_directory(path)?;
        }
        Ok(())
    }
}

fn validate_segment_header(file: &File, path: &Path, archive_id: u64) -> StaticFileResult<()> {
    let len = file
        .metadata()
        .map_err(|source| StaticFileError::io("read segment metadata", path, source))?
        .len();
    if len < u64::try_from(FILE_HEADER_LEN).expect("header length fits u64") {
        return Err(StaticFileError::invalid(
            0,
            "truncated archive segment header",
        ));
    }
    let mut header = [0u8; FILE_HEADER_LEN];
    read_exact_at(file, 0, &mut header)
        .map_err(|source| StaticFileError::io("read archive segment header", path, source))?;
    let actual_archive_id = validate_file_header(&header, 0)?;
    if actual_archive_id != archive_id {
        return Err(StaticFileError::invalid(
            0,
            format!(
                "archive segment identity mismatch: expected {archive_id:#018x}, got {actual_archive_id:#018x}"
            ),
        ));
    }
    Ok(())
}

fn segment_path(base_path: &Path, start_height: u32) -> StaticFileResult<PathBuf> {
    let file_name = base_path
        .file_name()
        .ok_or_else(|| StaticFileError::invalid(0, "archive path must have a file name"))?;
    let mut segment_name = file_name.to_os_string();
    segment_name.push(format!(".segment-{start_height:010}"));
    Ok(parent_directory(base_path).join(segment_name))
}

fn pending_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(PENDING_SUFFIX);
    PathBuf::from(name)
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn parse_segment_start(base_name: &OsStr, candidate: &OsStr) -> Option<u32> {
    let base = base_name.as_encoded_bytes();
    let candidate = candidate.as_encoded_bytes();
    let suffix = candidate.strip_prefix(base)?.strip_prefix(SEGMENT_MARKER)?;
    if suffix.len() != SEGMENT_HEIGHT_DIGITS || !suffix.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(suffix).ok()?.parse().ok()
}

fn is_pending_name(base_name: &OsStr, candidate: &OsStr) -> bool {
    let bytes = candidate.as_encoded_bytes();
    let Some(segment_suffix) = bytes
        .strip_prefix(base_name.as_encoded_bytes())
        .and_then(|suffix| suffix.strip_prefix(SEGMENT_MARKER))
    else {
        return false;
    };
    segment_suffix.len() == SEGMENT_HEIGHT_DIGITS + PENDING_SUFFIX.len()
        && segment_suffix[..SEGMENT_HEIGHT_DIGITS]
            .iter()
            .all(u8::is_ascii_digit)
        && &segment_suffix[SEGMENT_HEIGHT_DIGITS..] == PENDING_SUFFIX.as_bytes()
}

fn has_segment_marker(base_name: &OsStr, candidate: &OsStr) -> bool {
    candidate
        .as_encoded_bytes()
        .strip_prefix(base_name.as_encoded_bytes())
        .is_some_and(|suffix| suffix.starts_with(SEGMENT_MARKER))
}

#[cfg(test)]
mod tests {
    use super::{parse_segment_start, segment_path};
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn segment_names_are_height_addressed_and_strictly_parsed() {
        assert_eq!(
            segment_path(Path::new("data/ledger.static"), 42).expect("segment path"),
            Path::new("data/ledger.static.segment-0000000042")
        );
        assert_eq!(
            parse_segment_start(
                OsStr::new("ledger.static"),
                OsStr::new("ledger.static.segment-0000000042")
            ),
            Some(42)
        );
        assert_eq!(
            parse_segment_start(
                OsStr::new("ledger.static"),
                OsStr::new("ledger.static.segment-42")
            ),
            None
        );
    }
}
