/// The storage context used to read and write data in smart contracts.
pub struct StorageContext {
    /// The id of the contract that owns the context.
    pub id: i32,

    /// Indicates whether the context is read-only.
    pub is_read_only: bool,
}
