//! # neo-execution::iterators
//!
//! Iterator adapters exposed to contract execution and storage search.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `iterator`: contract iterator records.
//! - `iterator_interop`: iterator interop syscall handlers.
//! - `storage_iterator`: storage-backed iterator implementation.

/// Iterator trait definition.
pub mod iterator;
/// Iterator interop wrapper.
pub mod iterator_interop;
/// Storage iterator implementation.
pub mod storage_iterator;

pub use self::iterator_interop::IteratorInterop;
pub use self::storage_iterator::StorageIterator;
