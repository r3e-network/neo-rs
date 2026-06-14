use neo_primitives::BigDecimal;
use neo_primitives::UInt160;

/// Represents an output of a transfer.
/// Matches C# TransferOutput class exactly
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

    /// The object to be passed to the transfer method of NEP-17.
    /// Matches C# Data field
    pub data: Option<Box<dyn std::any::Any>>,
}
