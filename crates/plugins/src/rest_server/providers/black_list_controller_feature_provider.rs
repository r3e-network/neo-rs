// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RestServer.Providers.BlackListControllerFeatureProvider.

use crate::rest_server::rest_server_settings::RestServerSettings;

/// Filters controllers based on the blacklist defined in `RestServerSettings`.
///
/// The original C# implementation plugs into ASP.NET Core's controller
/// discovery pipeline. In the Rust server we surface the same behaviour as an
/// explicit helper that can be consulted while composing the warp filters.
#[derive(Debug, Clone)]
pub struct BlackListControllerFeatureProvider {
    settings: RestServerSettings,
}

impl BlackListControllerFeatureProvider {
    /// Creates a new provider snapshotting the current REST server settings.
    pub fn new() -> Self {
        Self {
            settings: RestServerSettings::current(),
        }
    }

    /// Reloads the provider settings from the global configuration singleton.
    pub fn refresh(&mut self) {
        self.settings = RestServerSettings::current();
    }

    /// Returns `true` when the supplied controller should be exposed.
    ///
    /// Controller names are matched case-insensitively, mirroring the C#
    /// comparison against `RestServerSettings.DisableControllers`.
    pub fn is_controller_allowed(&self, controller_name: &str) -> bool {
        !self
            .settings
            .disable_controllers
            .iter()
            .any(|entry| entry.eq_ignore_ascii_case(controller_name))
    }
}
