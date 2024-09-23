use serde::{Deserialize, Serialize};
use crate::core::mempoolevent;
use crate::network::payload;

// NotaryRequestEvent represents a P2PNotaryRequest event either added or removed
// from the notary payload pool.
#[derive(Serialize, Deserialize)]
pub struct NotaryRequestEvent {
    #[serde(rename = "type")]
    type_: mempoolevent::Type,
    #[serde(rename = "notaryrequest")]
    notary_request: Option<payload::P2PNotaryRequest>,
}
