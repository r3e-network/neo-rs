/// The reason a transaction was removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionRemovalReason {
    /// The transaction was rejected since it was the lowest priority transaction
    /// and the memory pool capacity was exceeded.
    CapacityExceeded,

    /// The transaction was rejected due to failing re-validation after a block was persisted.
    NoLongerValid,

    /// The transaction was rejected due to conflict with higher priority transactions
    /// with Conflicts attribute.
    Conflict,
}

impl From<TransactionRemovalReason> for u8 {
    fn from(reason: TransactionRemovalReason) -> Self {
        match reason {
            TransactionRemovalReason::CapacityExceeded => 0,
            TransactionRemovalReason::NoLongerValid => 1,
            TransactionRemovalReason::Conflict => 2,
        }
    }
}

impl TryFrom<u8> for TransactionRemovalReason {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TransactionRemovalReason::CapacityExceeded),
            1 => Ok(TransactionRemovalReason::NoLongerValid),
            2 => Ok(TransactionRemovalReason::Conflict),
            _ => Err(()),
        }
    }
}
