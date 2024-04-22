// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


pub mod engine;
pub mod builder;
pub mod interop;
pub mod opcode;


pub trait RunPrice {
    fn price(&self) -> u64;
}
