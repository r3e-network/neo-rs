// Copyright (C) 2015-2025 The Neo Project.
//
// Rust helper equivalent to `Neo.Plugins.RestServer.Binder.UInt160Binder`.

use crate::rest_server::rest_server_plugin::RestServerGlobals;
use crate::rest_server::RestServerUtility;
use neo_core::UInt160;

/// Utility that converts incoming strings into `UInt160` hashes using the current node settings.
pub struct UInt160Binder;

impl UInt160Binder {
    pub fn bind(value: &str) -> Option<UInt160> {
        if let Ok(hash) = value.parse::<UInt160>() {
            return Some(hash);
        }

        let system = RestServerGlobals::neo_system()?;
        RestServerUtility::convert_to_script_hash(value, system.settings()).ok()
    }
}
