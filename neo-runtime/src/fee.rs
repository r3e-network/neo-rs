// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_core::types::Fee;

pub const ECDSA_VERIFY_PRICE: usize = 1 << 15;


pub trait NetworkFee {
    fn network_fee(&self) -> Fee;
}

pub trait SystemFee {
    fn system_fee(&self) -> Fee;
}