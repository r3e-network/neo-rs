// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::string::String;

use crate::types::{H160, H256};

pub const ORACLE_RESPONSE_SCRIPT: &'static [u8] = b"TODO";

pub struct OracleRequest {
    pub original_tx_id: H256,
    pub gas_for_response: u64,
    pub url: String,
    pub filter: Option<String>,
    pub callback_contract: H160,
    pub callback_method: String,
    pub user_data: Vec<u8>,
}

pub struct OracleContract {
    //
}

impl OracleContract {
    pub fn last_designated(&self) -> Option<H160> {
        None // TODO
    }

    pub fn oracle_request(&self, _id: u64) -> Option<OracleRequest> {
        None // TODO
    }
}
