use neo_primitives::BigDecimal;
use neo_primitives::UInt160;

/// Represents an output of a transfer.
pub struct TransferOutput {
    /// The id of the asset to transfer.
    /// Matches C# AssetId field
    pub asset_id: UInt160,

    /// The amount of the asset to transfer.
    /// Matches C# Value field
    pub value: BigDecimal,

    /// The account to transfer to.
    /// Matches C# ScriptHash field
    pub script_hash: UInt160,
}
