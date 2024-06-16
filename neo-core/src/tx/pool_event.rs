// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use serde::{Deserialize, Serialize};
use crate::tx::Tx;


#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum TxEventType {
    #[serde(rename = "added")]
    TxAdded,

    #[serde(rename = "removed")]
    TxRemoved,
}


#[derive(Debug, Clone)]
pub struct TxEvent {
    pub typ: TxEventType,
    pub tx: Tx,
}
