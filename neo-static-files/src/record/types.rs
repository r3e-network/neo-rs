//! Protocol-blind finalized-height record types.

/// One opaque key/value row stored in a finalized-height frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticRow {
    key: Vec<u8>,
    value: Vec<u8>,
}

impl StaticRow {
    /// Creates an opaque row.
    #[must_use]
    pub const fn new(key: Vec<u8>, value: Vec<u8>) -> Self {
        Self { key, value }
    }

    /// Returns the row key.
    #[must_use]
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// Returns the row value.
    #[must_use]
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    pub(crate) fn into_parts(self) -> (Vec<u8>, Vec<u8>) {
        (self.key, self.value)
    }
}

/// Opaque immutable rows produced by one finalized block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticRecord {
    height: u32,
    rows: Vec<StaticRow>,
}

impl StaticRecord {
    /// Creates a finalized-height record.
    #[must_use]
    pub const fn new(height: u32, rows: Vec<StaticRow>) -> Self {
        Self { height, rows }
    }

    /// Returns the finalized block height.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns the opaque rows in this record.
    #[must_use]
    pub fn rows(&self) -> &[StaticRow] {
        &self.rows
    }

    pub(crate) fn into_parts(self) -> (u32, Vec<StaticRow>) {
        (self.height, self.rows)
    }
}
