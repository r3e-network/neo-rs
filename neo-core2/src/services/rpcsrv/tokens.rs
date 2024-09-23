use crate::neorpc::result;
use serde::{Deserialize, Serialize};

/// TokenTransfers is a generic type used to represent NEP-11 and NEP-17 transfers.
#[derive(Serialize, Deserialize)]
pub struct TokenTransfers {
    #[serde(rename = "sent")]
    pub sent: Vec<serde_json::Value>,
    #[serde(rename = "received")]
    pub received: Vec<serde_json::Value>,
    #[serde(rename = "address")]
    pub address: String,
}

/// Converts a NEP-17 transfer to a NEP-11 transfer by adding an ID.
pub fn nep17_transfer_to_nep11(t17: &result::NEP17Transfer, id: String) -> result::NEP11Transfer {
    result::NEP11Transfer {
        timestamp: t17.timestamp,
        asset: t17.asset.clone(),
        address: t17.address.clone(),
        id,
        amount: t17.amount,
        index: t17.index,
        notify_index: t17.notify_index,
        tx_hash: t17.tx_hash.clone(),
    }
}
