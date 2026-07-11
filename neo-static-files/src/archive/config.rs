//! Static-file compression and allocation limits.

use crate::format::{FRAME_FOOTER_LEN, FRAME_HEADER_LEN};
use crate::{StaticFileError, StaticFileResult};

/// Resource and compression limits for one static-file archive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StaticFileConfig {
    /// Zstandard compression level applied independently to each height frame.
    pub compression_level: i32,
    /// Number of decompressed frames retained by the existing `lru` cache.
    pub cache_capacity: usize,
    /// Maximum number of rows accepted in one finalized-height record.
    pub max_rows_per_record: usize,
    /// Maximum uncompressed value bytes accepted in one frame.
    pub max_uncompressed_record_bytes: usize,
    /// Maximum encoded frame size, including its row index and footer.
    pub max_frame_bytes: usize,
}

impl Default for StaticFileConfig {
    fn default() -> Self {
        Self {
            compression_level: 3,
            cache_capacity: 64,
            max_rows_per_record: 1_000_000,
            max_uncompressed_record_bytes: 256 * 1024 * 1024,
            max_frame_bytes: 384 * 1024 * 1024,
        }
    }
}

impl StaticFileConfig {
    /// Validates compression and allocation limits without opening a file.
    pub fn validate(self) -> StaticFileResult<()> {
        if !zstd::compression_level_range().contains(&self.compression_level) {
            return Err(StaticFileError::invalid(
                0,
                format!(
                    "compression_level {} is outside zstd range {:?}",
                    self.compression_level,
                    zstd::compression_level_range()
                ),
            ));
        }
        if self.cache_capacity == 0 {
            return Err(StaticFileError::invalid(
                0,
                "cache_capacity must be greater than zero",
            ));
        }
        if self.max_rows_per_record == 0
            || self.max_rows_per_record > usize::try_from(u32::MAX).unwrap_or(usize::MAX)
        {
            return Err(StaticFileError::invalid(
                0,
                "max_rows_per_record must fit in a non-zero u32",
            ));
        }
        if self.max_uncompressed_record_bytes == 0
            || self.max_uncompressed_record_bytes > usize::try_from(u32::MAX).unwrap_or(usize::MAX)
        {
            return Err(StaticFileError::invalid(
                0,
                "max_uncompressed_record_bytes must fit in a non-zero u32",
            ));
        }
        if self.max_frame_bytes < FRAME_HEADER_LEN + FRAME_FOOTER_LEN {
            return Err(StaticFileError::invalid(
                0,
                "max_frame_bytes is smaller than frame overhead",
            ));
        }
        Ok(())
    }
}
