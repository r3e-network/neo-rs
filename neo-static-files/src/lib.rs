//! # neo-static-files
//!
//! Append-only cold storage for finalized block, transaction, and receipt
//! bytes.
//!
//! ## Boundary
//!
//! This infrastructure crate owns append-only cold-file mechanics and must not
//! decide block validity, state roots, or sync policy.
//!
//! ## Contents
//!
//! - `neo-static-files`: append-only segment files, offsets, records, and recovery helpers.

use std::{
    any::Any,
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, RwLock},
};

use thiserror::Error;

const RECORD_MAGIC: [u8; 4] = *b"NSF1";
const RECORD_VERSION: u16 = 1;
const RECORD_HEADER_LEN: usize = 26;
const MAX_RECORD_PAYLOAD_BYTES: u64 = 512 * 1024 * 1024;

/// Byte offset inside a static-file segment.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Offset(u64);

impl Offset {
    /// First byte in a static-file segment.
    pub const ZERO: Self = Self(0);

    /// Construct an offset from a raw byte position.
    pub const fn new(bytes: u64) -> Self {
        Self(bytes)
    }

    /// Return the raw byte position.
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// Cold static-file segment.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Segment {
    /// Finalized block/header payloads.
    Blocks,
    /// Finalized transaction payloads.
    Transactions,
    /// Finalized execution receipts and application logs.
    Receipts,
    /// Optional archived state roots or trie snapshots.
    StateRoots,
}

impl Segment {
    fn file_name(self) -> &'static str {
        match self {
            Self::Blocks => "blocks.nsf",
            Self::Transactions => "transactions.nsf",
            Self::Receipts => "receipts.nsf",
            Self::StateRoots => "state-roots.nsf",
        }
    }
}

/// Result type for static-file operations.
pub type StaticFileResult<T> = Result<T, StaticFileError>;

/// Static-file storage errors.
#[derive(Debug, Error)]
pub enum StaticFileError {
    /// Filesystem operation failed.
    #[error("static-file IO failed for {path}: {source}")]
    Io {
        /// Path being operated on.
        path: PathBuf,
        /// Underlying IO error.
        #[source]
        source: io::Error,
    },
    /// Payload is too large for one static-file record.
    #[error("static-file payload has {actual} bytes, maximum is {max}")]
    PayloadTooLarge {
        /// Actual payload length.
        actual: u64,
        /// Maximum accepted payload length.
        max: u64,
    },
    /// Offset does not point at a valid record.
    #[error("invalid static-file offset {offset} in {path}")]
    InvalidOffset {
        /// Segment file path.
        path: PathBuf,
        /// Invalid offset.
        offset: u64,
    },
    /// Record header is malformed or belongs to an unsupported format.
    #[error("invalid static-file record header at offset {offset} in {path}")]
    InvalidHeader {
        /// Segment file path.
        path: PathBuf,
        /// Record offset.
        offset: u64,
    },
    /// Record checksum did not match the payload bytes.
    #[error("static-file checksum mismatch at offset {offset} in {path}")]
    ChecksumMismatch {
        /// Segment file path.
        path: PathBuf,
        /// Record offset.
        offset: u64,
    },
    /// Append offset arithmetic overflowed.
    #[error("static-file offset overflow while appending to {path}")]
    OffsetOverflow {
        /// Segment file path.
        path: PathBuf,
    },
    /// Internal mutex was poisoned by a previous panic.
    #[error("static-file writer lock poisoned")]
    LockPoisoned,
    /// Named static-file provider was not registered.
    #[error("static-file provider {provider:?} not found; available providers: {available:?}")]
    UnknownProvider {
        /// Requested provider name.
        provider: String,
        /// Registered provider names and aliases.
        available: Vec<String>,
    },
}

/// Location returned after appending a cold payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AppendRecord {
    /// Block height associated with this payload.
    pub block: u64,
    /// Byte offset where this record starts.
    pub offset: Offset,
    /// Payload length in bytes.
    pub payload_len: u64,
    /// First byte after this record; recovery truncates to this value.
    pub next_offset: Offset,
}

/// Appended payload location with its segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SegmentRecord {
    /// Segment that owns the payload.
    pub segment: Segment,
    /// Block height associated with this payload.
    pub block: u64,
    /// Byte offset where this record starts.
    pub offset: Offset,
    /// Payload length in bytes.
    pub payload_len: u64,
    /// First byte after this record; recovery truncates to this value.
    pub next_offset: Offset,
}

impl SegmentRecord {
    fn new(segment: Segment, record: AppendRecord) -> Self {
        Self {
            segment,
            block: record.block,
            offset: record.offset,
            payload_len: record.payload_len,
            next_offset: record.next_offset,
        }
    }
}

/// Append-only static files for cold finalized data.
pub trait StaticFiles: Send + Sync {
    /// Append one opaque payload to a segment.
    fn append(
        &self,
        segment: Segment,
        block: u64,
        payload: &[u8],
    ) -> StaticFileResult<AppendRecord>;

    /// Flush the segment file to durable storage.
    fn fsync(&self, segment: Segment) -> StaticFileResult<()>;

    /// Read a payload by the offset returned from [`StaticFiles::append`].
    fn read(&self, segment: Segment, offset: Offset) -> StaticFileResult<Vec<u8>>;

    /// Truncate a segment to a known committed end offset.
    fn truncate_to(&self, segment: Segment, offset: Offset) -> StaticFileResult<()>;

    /// Append a batch of cold payloads and fsync every segment touched by it.
    fn append_and_fsync_batch(
        &self,
        entries: &[(Segment, u64, &[u8])],
    ) -> StaticFileResult<Vec<SegmentRecord>> {
        let mut records = Vec::with_capacity(entries.len());
        let mut touched_segments = Vec::new();
        for (segment, block, payload) in entries {
            records.push(SegmentRecord::new(
                *segment,
                self.append(*segment, *block, payload)?,
            ));
            if !touched_segments.contains(segment) {
                touched_segments.push(*segment);
            }
        }
        for segment in touched_segments {
            self.fsync(segment)?;
        }
        Ok(records)
    }

    /// Truncate multiple segments to their committed end offsets.
    fn truncate_segments_to(&self, ends: &[(Segment, Offset)]) -> StaticFileResult<()> {
        for (segment, offset) in ends {
            self.truncate_to(*segment, *offset)?;
        }
        Ok(())
    }
}

/// Configuration used to open append-only static-file stores through a provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticFileConfig {
    /// Root directory for static-file segments.
    pub path: PathBuf,
}

impl StaticFileConfig {
    /// Construct static-file configuration from a root path.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

/// Provider that creates append-only static-file stores.
pub trait StaticFileProvider: Send + Sync + Any {
    /// Canonical provider name.
    fn name(&self) -> &str;

    /// Open static files rooted at `path`.
    fn open(&self, path: &Path) -> StaticFileResult<Arc<dyn StaticFiles>>;

    /// Open static files from a full static-file configuration.
    ///
    /// Providers that only need a path can rely on this default. Providers
    /// with backend-specific segment, compression, or recovery settings should
    /// override it so factory callers stay on the provider trait.
    fn open_with_config(&self, config: StaticFileConfig) -> StaticFileResult<Arc<dyn StaticFiles>> {
        self.open(&config.path)
    }

    /// Downcast support for provider tests and factory diagnostics.
    fn as_any(&self) -> &dyn Any;
}

/// Filesystem-backed static-file provider.
#[derive(Debug, Default)]
pub struct FileStaticFileProvider;

impl FileStaticFileProvider {
    /// Construct a filesystem-backed static-file provider.
    pub const fn new() -> Self {
        Self
    }
}

impl StaticFileProvider for FileStaticFileProvider {
    fn name(&self) -> &str {
        "file"
    }

    fn open(&self, path: &Path) -> StaticFileResult<Arc<dyn StaticFiles>> {
        FileStaticFiles::open(path).map(|store| Arc::new(store) as Arc<dyn StaticFiles>)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

const FILE_PROVIDER: &str = "file";

static STATIC_FILE_PROVIDERS: LazyLock<RwLock<HashMap<String, Arc<dyn StaticFileProvider>>>> =
    LazyLock::new(|| {
        let mut providers = HashMap::new();
        let file_provider = Arc::new(FileStaticFileProvider::new()) as Arc<dyn StaticFileProvider>;
        register_static_file_provider_aliases(&mut providers, file_provider, &[FILE_PROVIDER]);
        RwLock::new(providers)
    });

/// Factory for named static-file providers.
pub struct StaticFileFactory;

impl StaticFileFactory {
    /// Register a static-file provider by its canonical provider name.
    pub fn register_provider(provider: Arc<dyn StaticFileProvider>) -> StaticFileResult<()> {
        let mut providers = STATIC_FILE_PROVIDERS
            .write()
            .map_err(|_| StaticFileError::LockPoisoned)?;
        providers.insert(static_file_provider_key(provider.name()), provider);
        Ok(())
    }

    /// Return a registered provider by name or alias.
    pub fn get_static_file_provider(name: &str) -> Option<Arc<dyn StaticFileProvider>> {
        let providers = STATIC_FILE_PROVIDERS.read().ok()?;
        providers.get(&static_file_provider_key(name)).cloned()
    }

    /// Open a static-file store through a named provider.
    pub fn get_static_files(
        static_file_provider: &str,
        path: impl AsRef<Path>,
    ) -> StaticFileResult<Arc<dyn StaticFiles>> {
        Self::get_static_files_with_config(static_file_provider, StaticFileConfig::new(path))
    }

    /// Open a static-file store through a named provider and full configuration.
    pub fn get_static_files_with_config(
        static_file_provider: &str,
        config: StaticFileConfig,
    ) -> StaticFileResult<Arc<dyn StaticFiles>> {
        let key = static_file_provider_key(static_file_provider);
        let providers = STATIC_FILE_PROVIDERS
            .read()
            .map_err(|_| StaticFileError::LockPoisoned)?;
        let provider = providers
            .get(&key)
            .cloned()
            .ok_or_else(|| unknown_static_file_provider_error(static_file_provider, &providers))?;
        provider.open_with_config(config)
    }
}

fn static_file_provider_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn register_static_file_provider_aliases(
    providers: &mut HashMap<String, Arc<dyn StaticFileProvider>>,
    provider: Arc<dyn StaticFileProvider>,
    aliases: &[&str],
) {
    for alias in aliases {
        providers.insert(static_file_provider_key(alias), Arc::clone(&provider));
    }
}

fn unknown_static_file_provider_error(
    requested: &str,
    providers: &HashMap<String, Arc<dyn StaticFileProvider>>,
) -> StaticFileError {
    let mut available = providers.keys().cloned().collect::<Vec<_>>();
    available.sort_unstable();
    available.dedup();
    StaticFileError::UnknownProvider {
        provider: requested.to_string(),
        available,
    }
}

/// Filesystem-backed static files.
#[derive(Debug)]
pub struct FileStaticFiles {
    root: PathBuf,
    write_lock: Mutex<()>,
}

impl FileStaticFiles {
    /// Open or create static files under `root`.
    pub fn open(root: impl AsRef<Path>) -> StaticFileResult<Self> {
        let root = root.as_ref().to_path_buf();
        fs::create_dir_all(&root).map_err(|source| StaticFileError::Io {
            path: root.clone(),
            source,
        })?;
        Ok(Self {
            root,
            write_lock: Mutex::new(()),
        })
    }

    fn segment_path(&self, segment: Segment) -> PathBuf {
        self.root.join(segment.file_name())
    }

    fn open_for_append(&self, path: &Path) -> StaticFileResult<File> {
        OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(path)
            .map_err(|source| StaticFileError::Io {
                path: path.to_path_buf(),
                source,
            })
    }

    fn open_for_read(&self, path: &Path) -> StaticFileResult<File> {
        File::open(path).map_err(|source| StaticFileError::Io {
            path: path.to_path_buf(),
            source,
        })
    }
}

impl StaticFiles for FileStaticFiles {
    fn append(
        &self,
        segment: Segment,
        block: u64,
        payload: &[u8],
    ) -> StaticFileResult<AppendRecord> {
        let payload_len =
            u64::try_from(payload.len()).map_err(|_| StaticFileError::PayloadTooLarge {
                actual: u64::MAX,
                max: MAX_RECORD_PAYLOAD_BYTES,
            })?;
        if payload_len > MAX_RECORD_PAYLOAD_BYTES {
            return Err(StaticFileError::PayloadTooLarge {
                actual: payload_len,
                max: MAX_RECORD_PAYLOAD_BYTES,
            });
        }

        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| StaticFileError::LockPoisoned)?;
        let path = self.segment_path(segment);
        let mut file = self.open_for_append(&path)?;
        let offset = file
            .seek(SeekFrom::End(0))
            .map_err(|source| StaticFileError::Io {
                path: path.clone(),
                source,
            })?;
        let next_offset = offset
            .checked_add(u64::try_from(RECORD_HEADER_LEN).expect("header length fits u64"))
            .and_then(|offset| offset.checked_add(payload_len))
            .ok_or_else(|| StaticFileError::OffsetOverflow { path: path.clone() })?;

        let header = encode_header(block, payload_len, checksum(payload));
        file.write_all(&header)
            .map_err(|source| StaticFileError::Io {
                path: path.clone(),
                source,
            })?;
        file.write_all(payload)
            .map_err(|source| StaticFileError::Io {
                path: path.clone(),
                source,
            })?;
        file.flush().map_err(|source| StaticFileError::Io {
            path: path.clone(),
            source,
        })?;

        Ok(AppendRecord {
            block,
            offset: Offset::new(offset),
            payload_len,
            next_offset: Offset::new(next_offset),
        })
    }

    fn fsync(&self, segment: Segment) -> StaticFileResult<()> {
        let path = self.segment_path(segment);
        let file = self.open_for_append(&path)?;
        file.sync_all().map_err(|source| StaticFileError::Io {
            path: path.clone(),
            source,
        })
    }

    fn read(&self, segment: Segment, offset: Offset) -> StaticFileResult<Vec<u8>> {
        let path = self.segment_path(segment);
        let mut file = self.open_for_read(&path)?;
        file.seek(SeekFrom::Start(offset.as_u64()))
            .map_err(|source| StaticFileError::Io {
                path: path.clone(),
                source,
            })?;

        let mut header = [0_u8; RECORD_HEADER_LEN];
        file.read_exact(&mut header)
            .map_err(|source| match source.kind() {
                io::ErrorKind::UnexpectedEof => StaticFileError::InvalidOffset {
                    path: path.clone(),
                    offset: offset.as_u64(),
                },
                _ => StaticFileError::Io {
                    path: path.clone(),
                    source,
                },
            })?;
        let (_, payload_len, expected_checksum) = decode_header(&path, offset.as_u64(), &header)?;
        let payload_len_usize =
            usize::try_from(payload_len).map_err(|_| StaticFileError::PayloadTooLarge {
                actual: payload_len,
                max: MAX_RECORD_PAYLOAD_BYTES,
            })?;
        let mut payload = vec![0_u8; payload_len_usize];
        file.read_exact(&mut payload)
            .map_err(|source| match source.kind() {
                io::ErrorKind::UnexpectedEof => StaticFileError::InvalidOffset {
                    path: path.clone(),
                    offset: offset.as_u64(),
                },
                _ => StaticFileError::Io {
                    path: path.clone(),
                    source,
                },
            })?;
        if checksum(&payload) != expected_checksum {
            return Err(StaticFileError::ChecksumMismatch {
                path,
                offset: offset.as_u64(),
            });
        }
        Ok(payload)
    }

    fn truncate_to(&self, segment: Segment, offset: Offset) -> StaticFileResult<()> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| StaticFileError::LockPoisoned)?;
        let path = self.segment_path(segment);
        let file = self.open_for_append(&path)?;
        let len = file
            .metadata()
            .map_err(|source| StaticFileError::Io {
                path: path.clone(),
                source,
            })?
            .len();
        if offset.as_u64() > len {
            return Err(StaticFileError::InvalidOffset {
                path,
                offset: offset.as_u64(),
            });
        }
        file.set_len(offset.as_u64())
            .map_err(|source| StaticFileError::Io {
                path: path.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| StaticFileError::Io {
            path: path.clone(),
            source,
        })
    }
}

fn encode_header(block: u64, payload_len: u64, checksum: u32) -> [u8; RECORD_HEADER_LEN] {
    let mut header = [0_u8; RECORD_HEADER_LEN];
    header[0..4].copy_from_slice(&RECORD_MAGIC);
    header[4..6].copy_from_slice(&RECORD_VERSION.to_be_bytes());
    header[6..14].copy_from_slice(&block.to_be_bytes());
    header[14..22].copy_from_slice(&payload_len.to_be_bytes());
    header[22..26].copy_from_slice(&checksum.to_be_bytes());
    header
}

fn decode_header(
    path: &Path,
    offset: u64,
    header: &[u8; RECORD_HEADER_LEN],
) -> StaticFileResult<(u64, u64, u32)> {
    if header[0..4] != RECORD_MAGIC {
        return Err(StaticFileError::InvalidHeader {
            path: path.to_path_buf(),
            offset,
        });
    }
    let version = u16::from_be_bytes([header[4], header[5]]);
    if version != RECORD_VERSION {
        return Err(StaticFileError::InvalidHeader {
            path: path.to_path_buf(),
            offset,
        });
    }
    let block = u64::from_be_bytes([
        header[6], header[7], header[8], header[9], header[10], header[11], header[12], header[13],
    ]);
    let payload_len = u64::from_be_bytes([
        header[14], header[15], header[16], header[17], header[18], header[19], header[20],
        header[21],
    ]);
    if payload_len > MAX_RECORD_PAYLOAD_BYTES {
        return Err(StaticFileError::PayloadTooLarge {
            actual: payload_len,
            max: MAX_RECORD_PAYLOAD_BYTES,
        });
    }
    let expected_checksum = u32::from_be_bytes([header[22], header[23], header[24], header[25]]);
    Ok((block, payload_len, expected_checksum))
}

fn checksum(payload: &[u8]) -> u32 {
    payload.iter().fold(0x811c_9dc5_u32, |hash, byte| {
        hash.wrapping_mul(0x0100_0193) ^ u32::from(*byte)
    })
}
