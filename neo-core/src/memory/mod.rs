// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Memory management modules for Neo core system.
//!
//! Provides high-performance pooled object managers for reducing allocator pressure
//! during high-throughput transaction processing.

#[cfg(feature = "pooled-engine")]
pub mod vm_object_manager;

#[cfg(feature = "pooled-engine")]
pub use vm_object_manager::VmObjectManager;
