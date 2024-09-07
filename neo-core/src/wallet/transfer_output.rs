use crate::big_decimal::BigDecimal;
use crate::uint160::UInt160;

/// Represents an output of a transfer.
pub struct TransferOutput {
    /// The id of the asset to transfer.
    pub asset_id: UInt160,

    /// The amount of the asset to transfer.
    pub value: BigDecimal,

    /// The account to transfer to.
    pub script_hash: UInt160,

    /// The data to be passed to the transfer method of NEP-17.
    pub data: Option<Vec<u8>>,
}

impl TransferOutput {
    /// Creates a new TransferOutput instance.
    pub fn new(asset_id: UInt160, value: BigDecimal, script_hash: UInt160, data: Option<Vec<u8>>) -> Self {
        Self {
            asset_id,
            value,
            script_hash,
            data,
        }
    }
}
