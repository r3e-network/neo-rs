#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BatchOp {
    Put {
        column: super::ColumnId,
        key: Vec<u8>,
        value: Vec<u8>,
    },
    Delete {
        column: super::ColumnId,
        key: Vec<u8>,
    },
}

/// Ordered set of operations that should be applied atomically.
#[derive(Debug, Default, Clone)]
pub struct WriteBatch {
    ops: Vec<BatchOp>,
}

impl WriteBatch {
    #[inline]
    pub fn new() -> Self {
        Self { ops: Vec::new() }
    }

    #[inline]
    pub fn put(&mut self, column: super::ColumnId, key: Vec<u8>, value: Vec<u8>) {
        self.ops.push(BatchOp::Put { column, key, value });
    }

    #[inline]
    pub fn delete(&mut self, column: super::ColumnId, key: Vec<u8>) {
        self.ops.push(BatchOp::Delete { column, key });
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    #[inline]
    pub fn operations(&self) -> &[BatchOp] {
        &self.ops
    }

    #[inline]
    pub fn into_ops(self) -> Vec<BatchOp> {
        self.ops
    }
}
