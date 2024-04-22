// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


#[derive(Debug, Copy, Clone)]
#[repr(u8)]
pub enum TxRemovalReason {
    CapacityExceeded,
    NoLongerValid,
    Conflict,
}


pub struct TxPool {
    //
}