use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use crate::error::{IndexerError, IndexerResult};
use crate::indexer::Indexer;
use crate::model::IndexerSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MutationPersistenceMode {
    None,
    JsonFile,
    Store,
}

impl MutationPersistenceMode {
    pub(super) fn is_persistent(self) -> bool {
        !matches!(self, Self::None)
    }
}

pub(super) enum PendingPersistence {
    JsonSnapshot(IndexerSnapshot),
    StoreDelta {
        previous: IndexerSnapshot,
        current: IndexerSnapshot,
    },
}

pub(super) fn read_snapshot(path: &Path) -> IndexerResult<Indexer> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Indexer::new()),
        Err(source) => {
            return Err(IndexerError::SnapshotRead {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let snapshot = serde_json::from_reader::<_, IndexerSnapshot>(file).map_err(|source| {
        IndexerError::SnapshotDecode {
            path: path.to_path_buf(),
            source,
        }
    })?;
    Indexer::from_snapshot(snapshot)
}

pub(super) fn write_snapshot(path: &Path, snapshot: &IndexerSnapshot) -> IndexerResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| IndexerError::SnapshotWrite {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let temp_path = temporary_snapshot_path(path);
    let write_result = (|| {
        let mut file = File::create(&temp_path).map_err(|source| IndexerError::SnapshotWrite {
            path: temp_path.clone(),
            source,
        })?;
        serde_json::to_writer_pretty(&mut file, snapshot).map_err(|source| {
            IndexerError::SnapshotEncode {
                path: temp_path.clone(),
                source,
            }
        })?;
        file.write_all(b"\n")
            .and_then(|()| file.sync_all())
            .map_err(|source| IndexerError::SnapshotWrite {
                path: temp_path.clone(),
                source,
            })?;
        Ok(())
    })();
    if let Err(err) = write_result {
        remove_temporary_snapshot(&temp_path);
        return Err(err);
    }

    if let Err(source) = fs::rename(&temp_path, path) {
        remove_temporary_snapshot(&temp_path);
        return Err(IndexerError::SnapshotWrite {
            path: path.to_path_buf(),
            source,
        });
    }
    Ok(())
}

pub(super) fn temporary_snapshot_path(path: &Path) -> PathBuf {
    let mut temp_path = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map_or_else(|| "tmp".to_string(), |value| format!("{value}.tmp"));
    temp_path.set_extension(extension);
    temp_path
}

fn remove_temporary_snapshot(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(_) => {}
    }
}
