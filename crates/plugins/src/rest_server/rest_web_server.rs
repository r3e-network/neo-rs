//! Stub implementation of the REST web server used by the plugin.
//! This will later host the full HTTP stack mirrored from the C# version.

use neo_extensions::ExtensionResult;

use super::rest_server_settings::RestServerSettings;

#[derive(Debug, Clone)]
pub struct RestWebServer {
    settings: RestServerSettings,
}

impl RestWebServer {
    pub fn new(settings: RestServerSettings) -> Self {
        Self { settings }
    }

    pub async fn start(&self) -> ExtensionResult<()> {
        // TODO: Boot the HTTP server implementation.
        let _ = &self.settings;
        Ok(())
    }

    pub async fn stop(&self) -> ExtensionResult<()> {
        // TODO: Shut down the HTTP server implementation.
        Ok(())
    }
}
