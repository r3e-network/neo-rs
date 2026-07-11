//! Portable positioned file I/O and directory durability helpers.

use std::fs::File;
use std::path::Path;

#[cfg(not(any(unix, windows)))]
use std::io::{Read, Seek, SeekFrom, Write};

use crate::{StaticFileError, StaticFileResult};

#[cfg(unix)]
pub(super) fn sync_parent_directory(path: &Path) -> StaticFileResult<()> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| StaticFileError::io("sync parent directory", parent, source))
}

#[cfg(not(unix))]
pub(super) fn sync_parent_directory(_path: &Path) -> StaticFileResult<()> {
    // Windows and other supported targets do not expose portable directory
    // fsync through std. The archive file itself is still sync_all'd.
    Ok(())
}

#[cfg(unix)]
pub(super) fn read_exact_at(
    file: &File,
    mut offset: u64,
    mut buffer: &mut [u8],
) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;

    while !buffer.is_empty() {
        let read = file.read_at(buffer, offset)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "positioned read reached EOF",
            ));
        }
        offset += u64::try_from(read).expect("usize fits u64 on supported targets");
        buffer = &mut buffer[read..];
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn read_exact_at(
    file: &File,
    mut offset: u64,
    mut buffer: &mut [u8],
) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;

    while !buffer.is_empty() {
        let read = file.seek_read(buffer, offset)?;
        if read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "positioned read reached EOF",
            ));
        }
        offset += u64::try_from(read).expect("usize fits u64 on supported targets");
        buffer = &mut buffer[read..];
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn read_exact_at(file: &File, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(offset))?;
    reader.read_exact(buffer)
}

#[cfg(unix)]
pub(super) fn write_all_at(file: &File, mut offset: u64, mut buffer: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::FileExt;

    while !buffer.is_empty() {
        let written = file.write_at(buffer, offset)?;
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "positioned write made no progress",
            ));
        }
        offset += u64::try_from(written).expect("usize fits u64 on supported targets");
        buffer = &buffer[written..];
    }
    Ok(())
}

#[cfg(windows)]
pub(super) fn write_all_at(file: &File, mut offset: u64, mut buffer: &[u8]) -> std::io::Result<()> {
    use std::os::windows::fs::FileExt;

    while !buffer.is_empty() {
        let written = file.seek_write(buffer, offset)?;
        if written == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "positioned write made no progress",
            ));
        }
        offset += u64::try_from(written).expect("usize fits u64 on supported targets");
        buffer = &buffer[written..];
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
pub(super) fn write_all_at(file: &File, offset: u64, buffer: &[u8]) -> std::io::Result<()> {
    let mut writer = file.try_clone()?;
    writer.seek(SeekFrom::Start(offset))?;
    writer.write_all(buffer)
}
