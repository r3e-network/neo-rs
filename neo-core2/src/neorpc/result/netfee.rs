use serde::{Deserialize, Serialize};

// NetworkFee represents a result of calculatenetworkfee RPC call.
#[derive(Serialize, Deserialize)]
pub struct NetworkFee {
    #[serde(rename = "networkfee", with = "serde_with::rust::display_fromstr")]
    value: i64,
}
