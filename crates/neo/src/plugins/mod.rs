//! Plugin support mirroring the public surface of the C# `Neo.Plugins`
//! namespace.  The Rust port focuses on the structural pieces consumed by the
//! rest of the node (metadata, settings, registry) while deferring the
//! filesystem watching and dynamic loading logic to future work.

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use once_cell::sync::Lazy;

/// Matches `Neo.Plugins.UnhandledExceptionPolicy` from the C# node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnhandledExceptionPolicy {
    Ignore,
    StopPlugin,
    #[default]
    StopNode,
}

/// Equivalent to `Neo.Plugins.IPluginSettings` providing per-plugin
/// configuration such as the unhandled exception policy.
pub trait PluginSettings: Send + Sync {
    fn exception_policy(&self) -> UnhandledExceptionPolicy {
        UnhandledExceptionPolicy::StopNode
    }
}

/// Metadata describing a plugin instance.  Mirrors the properties exposed by
/// the C# `Plugin` base class.
#[derive(Clone, Debug, Default)]
pub struct PluginMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    pub root_path: PathBuf,
    pub config_file: PathBuf,
    pub library_path: PathBuf,
}

impl PluginMetadata {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let root_path = default_plugins_directory().join(&name);
        let config_file = root_path.join("config.json");
        let library_path = root_path.join(format!("{name}.dll"));

        Self {
            name,
            description: String::new(),
            version: "0.0.0".to_string(),
            root_path,
            config_file,
            library_path,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }
}

/// Convenience helper for formatting metadata in logs.
impl fmt::Display for PluginMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} v{} (config: {}, library: {})",
            self.name,
            self.version,
            self.config_file.display(),
            self.library_path.display()
        )
    }
}

/// Simplified analogue of the C# `Plugin` abstract class.
pub trait Plugin: PluginSettings + fmt::Debug {
    /// Returns the metadata that describes this plugin instance.
    fn metadata(&self) -> &PluginMetadata;

    /// Called after the plugin is registered allowing custom initialisation.
    fn configure(&self) {}

    /// Invoked when the system shuts down or the plugin is removed.
    fn dispose(&self) {}
}

/// Global registry of loaded plugins, mirroring the static list maintained by
/// the C# runtime.
static PLUGINS: Lazy<RwLock<Vec<Arc<dyn Plugin>>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// Registers a plugin instance with the global registry. The plugin's
/// `configure` hook is executed immediately.
pub fn register_plugin(plugin: Arc<dyn Plugin>) {
    plugin.configure();
    PLUGINS
        .write()
        .expect("plugins registry poisoned")
        .push(plugin);
}

/// Removes all registered plugins, invoking `dispose` for each one.
pub fn clear_plugins() {
    let mut plugins = PLUGINS.write().expect("plugins registry poisoned");
    for plugin in plugins.drain(..) {
        plugin.dispose();
    }
}

/// Returns a snapshot of the currently registered plugins.
pub fn registered_plugins() -> Vec<Arc<dyn Plugin>> {
    PLUGINS
        .read()
        .expect("plugins registry poisoned")
        .iter()
        .cloned()
        .collect()
}

/// Computes the default plugins directory similar to the C# implementation
/// (`AppContext.BaseDirectory/Plugins`).
fn default_plugins_directory() -> PathBuf {
    application_root()
        .map(|root| root.join("Plugins"))
        .unwrap_or_else(|| PathBuf::from("Plugins"))
}

fn application_root() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}
