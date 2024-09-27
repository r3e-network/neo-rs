use crate::big_decimal::BigDecimal;
use neo_type::H160;

/// Represents an output of a transfer.
pub struct TransferOutput {
    /// The id of the asset to transfer.
    pub asset_id: H160,

    /// The amount of the asset to transfer.
    pub value: BigDecimal,

    /// The account to transfer to.
    pub script_hash: H160,

    /// The data to be passed to the transfer method of NEP-17.
    pub data: Option<Vec<u8>>,
}

impl TransferOutput {
    /// Creates a new TransferOutput instance.
    pub fn new(asset_id: H160, value: BigDecimal, script_hash: H160, data: Option<Vec<u8>>) -> Self {
        Self {
            asset_id,
            value,
            script_hash,
            data,
        }
    }
}
