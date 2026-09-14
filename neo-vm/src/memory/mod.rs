// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Memory management modules for the Neo Virtual Machine.
//!
//! This module provides high-performance memory allocation strategies for VM execution,
//! focusing on reducing GC pressure and malloc/free overhead for short-lived objects.

pub mod arena_pool;
