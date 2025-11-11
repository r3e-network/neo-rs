use std::fmt;

/// Named identifier for a column family.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ColumnId(pub &'static str);

impl ColumnId {
    pub const fn new(name: &'static str) -> Self {
        Self(name)
    }

    #[inline]
    pub const fn name(self) -> &'static str {
        self.0
    }
}

impl From<&'static str> for ColumnId {
    #[inline]
    fn from(value: &'static str) -> Self {
        ColumnId::new(value)
    }
}

impl fmt::Display for ColumnId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Marker trait linking a type to a column identifier.
pub trait Column {
    const ID: ColumnId;
}
