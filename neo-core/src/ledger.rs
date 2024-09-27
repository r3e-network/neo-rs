// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::store::{BlockStates, ChainStates};

// Ledger for Stored Chain States
pub trait Ledger: BlockStates + ChainStates {
    //
}
