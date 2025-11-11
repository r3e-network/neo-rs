/// Pending transaction tracked by the runtime pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingTransaction {
    pub id: String,
    pub fee: u64,
    pub size_bytes: u32,
}

impl PendingTransaction {
    pub fn new(id: impl Into<String>, fee: u64, size_bytes: u32) -> Self {
        Self {
            id: id.into(),
            fee,
            size_bytes,
        }
    }

    pub fn fee_per_byte(&self) -> f64 {
        if self.size_bytes == 0 {
            return self.fee as f64;
        }
        self.fee as f64 / self.size_bytes as f64
    }
}
