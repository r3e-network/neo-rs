//! File writer construction and size-based archive rotation.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

use crate::node::config::LoggingSection;

pub(super) fn open_file_writer(
    path: &Path,
    config: &LoggingSection,
) -> anyhow::Result<(NonBlocking, WorkerGuard)> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(directory) = directory {
        std::fs::create_dir_all(directory)
            .with_context(|| format!("creating log directory {}", directory.display()))?;
    }
    let file_name = path
        .file_name()
        .context("[logging].file_path must include a file name")?;
    if let Some(max_size) = config.max_file_size_bytes()? {
        let writer = SizeRotatingFileWriter::open(path, max_size, config.max_rotated_files())?;
        Ok(tracing_appender::non_blocking(writer))
    } else {
        let appender = tracing_appender::rolling::never(
            directory.unwrap_or_else(|| Path::new(".")),
            file_name,
        );
        Ok(tracing_appender::non_blocking(appender))
    }
}

pub(super) struct SizeRotatingFileWriter {
    path: PathBuf,
    file: Option<File>,
    current_size: u64,
    max_size: u64,
    max_archives: usize,
}

impl SizeRotatingFileWriter {
    pub(super) fn open(path: &Path, max_size: u64, max_archives: usize) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        if existing_len(path)? >= max_size {
            rotate_paths(path, max_archives)?;
        }
        let file = open_append(path)?;
        let current_size = existing_len(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Some(file),
            current_size,
            max_size,
            max_archives,
        })
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
        }
        rotate_paths(&self.path, self.max_archives)?;
        self.file = Some(open_append(&self.path)?);
        self.current_size = 0;
        Ok(())
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "rotating log file is closed"))
    }
}

impl Write for SizeRotatingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.current_size > 0
            && self
                .current_size
                .saturating_add(u64::try_from(buf.len()).unwrap_or(u64::MAX))
                > self.max_size
        {
            self.rotate()?;
        }
        let written = self.file_mut()?.write(buf)?;
        self.current_size = self.current_size.saturating_add(written as u64);
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file_mut()?.flush()
    }
}

fn open_append(path: &Path) -> io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

fn existing_len(path: &Path) -> io::Result<u64> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err),
    }
}

fn rotate_paths(path: &Path, max_archives: usize) -> io::Result<()> {
    if max_archives == 0 {
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        return Ok(());
    }

    let oldest = archive_path(path, max_archives);
    if oldest.exists() {
        std::fs::remove_file(oldest)?;
    }
    for index in (1..max_archives).rev() {
        let from = archive_path(path, index);
        if from.exists() {
            std::fs::rename(from, archive_path(path, index + 1))?;
        }
    }
    if path.exists() {
        std::fs::rename(path, archive_path(path, 1))?;
    }
    Ok(())
}

pub(super) fn archive_path(path: &Path, index: usize) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "neo-node.log".into());
    path.with_file_name(format!("{file_name}.{index}"))
}
