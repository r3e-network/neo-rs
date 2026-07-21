//! Stable segment identities and positioned pack locations.

use std::ffi::OsStr;
use std::fmt;

/// Segment-header format emitted and accepted by this pack engine.
///
/// The header itself is introduced with the single-segment store foundation;
/// rotation and multi-segment routing are owned by the segment layer.
pub const PACK_SEGMENT_FORMAT_VERSION: u32 = 1;
/// Fixed byte length of every version-1 segment header.
pub const PACK_SEGMENT_HEADER_LEN: u64 = 64;

const SEGMENT_FILE_PREFIX: &str = "frames-";
const SEGMENT_FILE_SUFFIX: &str = ".pack";
const SEGMENT_ID_DIGITS: usize = 20;

/// Stable, monotonically increasing identity of one node-pack segment.
///
/// Segment zero is the first segment in a store. Identities are persisted in
/// frame locations and canonical commit horizons; filesystem discovery must
/// never infer identity from directory iteration order.
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct PackSegmentId(u64);

impl PackSegmentId {
    /// Identity of the first segment in a new store.
    pub const INITIAL: Self = Self(0);

    /// Constructs a segment identity from its durable integer representation.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the durable integer representation.
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns the next segment identity, or `None` at the integer limit.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Returns the canonical filename for this segment.
    pub fn file_name(self) -> String {
        format!(
            "{SEGMENT_FILE_PREFIX}{:0SEGMENT_ID_DIGITS$}{SEGMENT_FILE_SUFFIX}",
            self.0
        )
    }

    /// Parses a canonical segment filename.
    ///
    /// Non-canonical names are ignored by discovery rather than being
    /// accepted as aliases for the same durable segment identity.
    pub fn from_file_name(name: &OsStr) -> Option<Self> {
        let bytes = name.as_encoded_bytes();
        let digits = bytes
            .strip_prefix(SEGMENT_FILE_PREFIX.as_bytes())?
            .strip_suffix(SEGMENT_FILE_SUFFIX.as_bytes())?;
        if digits.len() != SEGMENT_ID_DIGITS || !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let value = std::str::from_utf8(digits).ok()?.parse().ok()?;
        let id = Self(value);
        (id.file_name().as_bytes() == bytes).then_some(id)
    }
}

impl fmt::Debug for PackSegmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("PackSegmentId")
            .field(&self.0)
            .finish()
    }
}

impl fmt::Display for PackSegmentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Byte position inside one identified pack segment.
///
/// Offsets are segment-relative. This prevents a valid offset from one file
/// being confused with the same numeric offset in another segment.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PackPosition {
    segment: PackSegmentId,
    offset: u64,
}

impl PackPosition {
    /// Constructs a segment-relative byte position.
    pub const fn new(segment: PackSegmentId, offset: u64) -> Self {
        Self { segment, offset }
    }

    /// Returns the segment containing this position.
    pub const fn segment(self) -> PackSegmentId {
        self.segment
    }

    /// Returns the byte offset relative to the segment start.
    pub const fn offset(self) -> u64 {
        self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_file_names_round_trip_canonically() {
        for id in [
            PackSegmentId::INITIAL,
            PackSegmentId::new(42),
            PackSegmentId::new(u64::MAX),
        ] {
            let name = id.file_name();
            assert_eq!(PackSegmentId::from_file_name(OsStr::new(&name)), Some(id));
        }
    }

    #[test]
    fn segment_file_name_parser_rejects_aliases_and_malformed_names() {
        for name in [
            "frames.pack",
            "frames-0.pack",
            "frames-00000000000000000000.pack.tmp",
            "frames-0000000000000000000x.pack",
            "Frames-00000000000000000000.pack",
        ] {
            assert_eq!(PackSegmentId::from_file_name(OsStr::new(name)), None);
        }
    }

    #[test]
    fn positions_bind_equal_offsets_to_their_segment() {
        let first = PackPosition::new(PackSegmentId::INITIAL, 4096);
        let second = PackPosition::new(PackSegmentId::new(1), 4096);
        assert_ne!(first, second);
        assert_eq!(first.segment(), PackSegmentId::INITIAL);
        assert_eq!(first.offset(), 4096);
    }

    #[test]
    fn segment_identity_overflow_is_explicit() {
        assert_eq!(
            PackSegmentId::INITIAL.checked_next(),
            Some(PackSegmentId::new(1))
        );
        assert_eq!(PackSegmentId::new(u64::MAX).checked_next(), None);
    }
}
