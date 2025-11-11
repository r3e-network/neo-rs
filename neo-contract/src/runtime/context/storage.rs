use neo_base::Bytes;
use neo_store::ColumnId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageContext {
    column: ColumnId,
}

impl StorageContext {
    pub const fn new(column: ColumnId) -> Self {
        Self { column }
    }

    pub const fn column(self) -> ColumnId {
        self.column
    }

    pub fn to_bytes(self) -> Bytes {
        Bytes::from(self.column.name().as_bytes().to_vec())
    }
}

impl Default for StorageContext {
    fn default() -> Self {
        StorageContext::new(ColumnId::new("contract"))
    }
}
