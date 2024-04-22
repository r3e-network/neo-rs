// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod bytes;
pub mod defer;
pub mod encoding;
pub mod errors;
pub mod hash;
pub mod math;
pub mod mem;


pub fn byzantine_honest_quorum(n: u32) -> u32 { n - (n - 1) / 3 }

pub fn byzantine_failure_quorum(n: u32) -> u32 { (n - 1) / 3 }

// #[cfg(all(not(test), not(feature = "std")))]
// #[panic_handler]
// fn on_panic(_info: &core::panic::PanicInfo) -> ! {
//     cfg_if::cfg_if! {
//         if #[cfg(target_arch = "wasm32")] {
//             core::arch::wasm32::unreachable();
//         } else {
//             unreachable!();
//         }
//     }
// }