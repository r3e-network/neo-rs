use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{
    StaticFileArchive, StaticFileArchiveFactory, StaticFileConfig, StaticFileProviderFactory,
    StaticRow,
};

pub(super) fn row(key: &[u8], value: &[u8]) -> StaticRow {
    StaticRow::new(key.to_vec(), value.to_vec())
}

pub(super) fn test_config() -> StaticFileConfig {
    StaticFileConfig {
        compression_level: 1,
        cache_capacity: 4,
        ..StaticFileConfig::default()
    }
}

pub(super) fn open_archive(path: &Path) -> StaticFileArchive {
    StaticFileArchiveFactory::new(test_config())
        .open(path)
        .expect("open archive")
}

pub(super) fn corrupt_payload(path: &Path, frame_offset: u64) -> u64 {
    let (payload_offset, frame_end) = frame_layout(path, frame_offset);
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("open archive for corruption");
    file.seek(SeekFrom::Start(payload_offset))
        .expect("seek payload");
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).expect("read payload byte");
    byte[0] ^= 0x80;
    file.seek(SeekFrom::Start(payload_offset))
        .expect("seek payload");
    file.write_all(&byte).expect("corrupt payload byte");
    file.sync_all().expect("sync corruption");
    frame_end
}

fn frame_layout(path: &Path, frame_offset: u64) -> (u64, u64) {
    let mut file = OpenOptions::new()
        .read(true)
        .open(path)
        .expect("open archive");
    file.seek(SeekFrom::Start(frame_offset))
        .expect("seek frame header");
    let mut header_bytes = [0u8; crate::format::FRAME_HEADER_LEN];
    file.read_exact(&mut header_bytes)
        .expect("read frame header");
    let header = crate::format::decode_frame_header(&header_bytes, frame_offset)
        .expect("decode frame header");
    let payload_offset = frame_offset
        + u64::try_from(crate::format::FRAME_HEADER_LEN).expect("header length")
        + u64::from(header.index_len);
    (payload_offset, frame_offset + header.frame_len)
}
