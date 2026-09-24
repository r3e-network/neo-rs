// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Memory management modules for the Neo Virtual Machine.
//!
//! This module provides high-performance memory allocation strategies for VM execution,
//! focusing on reducing GC pressure and malloc/free overhead for short-lived objects.
//!
//! ## Object Pool Pattern
//!
//! The `ObjectPool` type pre-allocates frequently-used objects and reuses them
//! across transactions, significantly reducing allocator pressure at ~10k TPS.
//!
//! ```rust,no_run
//! use neo_vm::memory::ObjectPool;
//!
//! // Pre-warm a pool of 64 u64 slots
//! let pool = ObjectPool::<u64>::new(64);
//!
//! // Borrow an item (zero allocation if pooled)
//! let item = pool.borrow();
//! drop(item); // Automatically returns to pool on drop
//! ```
//!
//! ## Pooled Containers
//!
//! Reusable containers (`PooledVec`, `PooledHashMap`) prevent reallocations during
//! typical transaction processing by maintaining fixed capacity buffers.

pub mod arena_pool;
pub mod object_pool;
pub mod pooled_containers;

pub use object_pool::{ObjectPool, PooledRef};
pub use pooled_containers::{PooledHashMap, PooledVec};
