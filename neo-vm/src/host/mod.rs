//! Shared host-boundary helpers.

mod syscall;

pub use syscall::{interop_hash, syscall_arg_count};
