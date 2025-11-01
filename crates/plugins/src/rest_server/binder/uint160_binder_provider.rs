// Copyright (C) 2015-2025 The Neo Project.
//
// Rust analogue of `Neo.Plugins.RestServer.Binder.NeoBinderProvider`.

use super::uint160_binder::UInt160Binder;
use neo_core::UInt160;

pub struct UInt160BinderProvider;

impl UInt160BinderProvider {
    /// Attempts to parse the supplied value into a `UInt160`, returning `None` when conversion fails.
    pub fn bind(value: &str) -> Option<UInt160> {
        UInt160Binder::bind(value)
    }
}
