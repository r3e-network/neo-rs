use neo_base::Bytes;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageFindItem {
    pub kind: StorageFindItemKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageFindItemKind {
    Key(Bytes),
    Value(Bytes),
    KeyValue { key: Bytes, value: Bytes },
}

impl StorageFindItem {
    pub fn key(key: Bytes) -> Self {
        Self {
            kind: StorageFindItemKind::Key(key),
        }
    }

    pub fn value(value: Bytes) -> Self {
        Self {
            kind: StorageFindItemKind::Value(value),
        }
    }

    pub fn key_value(key: Bytes, value: Bytes) -> Self {
        Self {
            kind: StorageFindItemKind::KeyValue { key, value },
        }
    }
}
