// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_type::H160;
use crate::tx::AttrType;

pub struct PolicyContract {
    //
}

impl PolicyContract {
    pub fn is_blocked_account(&self, _account: &H160) -> bool {
        false // TODO
    }

    pub fn tx_attr_fee(&self, _attr: AttrType) -> u64 {
        0 // TODO
    }

    pub fn netfee_perbyte(&self) -> u64 {
        0 // TODO
    }

    pub fn exec_fee_factor(&self) -> u64 {
        0 // TODO
    }
}
