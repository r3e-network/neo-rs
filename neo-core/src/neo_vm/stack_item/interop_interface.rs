// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Interop interface trait for the Neo Virtual Machine.

use std::fmt;

/// A trait for interop interfaces that can be wrapped by a `stack_item`.
pub trait InteropInterface: fmt::Debug + Send + Sync {
    /// Gets the type of the interop interface.
    fn interface_type(&self) -> &str;

    /// Allows downcasting to concrete types
    fn as_any(&self) -> &dyn std::any::Any;
}
