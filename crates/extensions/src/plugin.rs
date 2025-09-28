//! Plugin system for Neo extensions, mirroring the behaviour of
//! `Neo.Extensions.Plugin` in the C# codebase.

use crate::error::{ExtensionError, ExtensionResult};
use async_trait::async_trait;
use neo_core::NeoSystem;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Trait implemented by all Neo plugins.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Retrieve static plugin metadata.
    fn info(&self) -> &PluginInfo;

    /// Initialise the plugin (load configuration, prepare state, etc.).
    async fn initialize(&mut self, context: &PluginContext) -> ExtensionResult<()>;

    /// Start the plugin after initialisation completes.
    async fn start(&mut self) -> ExtensionResult<()>;

    /// Stop the plugin and release resources.
    async fn stop(&mut self) -> ExtensionResult<()>;

    /// Handle an event broadcast by the runtime.
    async fn handle_event(&mut self, event: &PluginEvent) -> ExtensionResult<()>;

    /// Optional configuration schema hook.
    fn config_schema(&self) -> Option<serde_json::Value> {
        None
    }

    /// Optional dynamic configuration update hook.
    async fn update_config(&mut self, _config: serde_json::Value) -> ExtensionResult<()> {
        Ok(())
    }
}

/// Metadata describing a plugin (matches the C# `Plugin` base class contract).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub dependencies: Vec<String>,
    pub min_neo_version: String,
    pub category: PluginCategory,
    pub priority: i32,
}

/// Categories used for grouping plugins.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PluginCategory {
    Core,
    Network,
    Consensus,
    Rpc,
    Wallet,
    Storage,
    Utility,
    Custom(String),
}

/// Context supplied to plugins during initialisation.
#[derive(Debug, Clone)]
pub struct PluginContext {
    pub neo_version: String,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub shared_data: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

/// Events that can be broadcast to plugins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PluginEvent {
    /// Equivalent to `Plugin.OnSystemLoaded` in C#, providing the `NeoSystem`.
    NodeStarted { system: Arc<NeoSystem> },
    /// Node stopping notification.
    NodeStopping,
    /// New block received event.
    BlockReceived {
        block_hash: String,
        block_height: u32,
    },
    /// New transaction received event.
    TransactionReceived { tx_hash: String },
    /// Consensus state change notification.
    ConsensusStateChanged { state: String },
    /// RPC request processed by the runtime.
    RpcRequest {
        method: String,
        params: serde_json::Value,
    },
    /// Custom application-specific event.
    Custom {
        event_type: String,
        data: serde_json::Value,
    },
}

/// Manager responsible for orchestrating plugin lifecycle.
pub struct PluginManager {
    plugins: HashMap<String, Box<dyn Plugin>>,
    plugin_order: Vec<String>,
    context: PluginContext,
    is_initialized: bool,
}

impl PluginManager {
    /// Create a new plugin manager instance.
    pub fn new(context: PluginContext) -> Self {
        Self {
            plugins: HashMap::new(),
            plugin_order: Vec::new(),
            context,
            is_initialized: false,
        }
    }

    /// Register a plugin with the manager.
    pub fn register_plugin(&mut self, plugin: Box<dyn Plugin>) -> ExtensionResult<()> {
        let info = plugin.info().clone();

        if self.plugins.contains_key(&info.name) {
            return Err(ExtensionError::PluginAlreadyExists(info.name));
        }

        for dep in &info.dependencies {
            if !self.plugins.contains_key(dep) {
                return Err(ExtensionError::MissingDependency {
                    plugin: info.name.clone(),
                    dependency: dep.clone(),
                });
            }
        }

        info!("Registering plugin: {} v{}", info.name, info.version);

        let insert_pos = self
            .plugin_order
            .iter()
            .position(|name| {
                self.plugins
                    .get(name)
                    .map(|p| p.info().priority < info.priority)
                    .unwrap_or(false)
            })
            .unwrap_or(self.plugin_order.len());

        self.plugin_order.insert(insert_pos, info.name.clone());
        self.plugins.insert(info.name, plugin);

        Ok(())
    }

    /// Initialise all registered plugins.
    pub async fn initialize_all(&mut self) -> ExtensionResult<()> {
        if self.is_initialized {
            return Ok(());
        }

        info!("Initializing {} plugins", self.plugins.len());

        for plugin_name in &self.plugin_order.clone() {
            if let Some(plugin) = self.plugins.get_mut(plugin_name) {
                info!("Initializing plugin: {}", plugin_name);
                plugin.initialize(&self.context).await.map_err(|err| {
                    error!("Failed to initialize plugin {}: {}", plugin_name, err);
                    err
                })?;
            }
        }

        self.is_initialized = true;
        info!("All plugins initialized successfully");
        Ok(())
    }

    /// Start all registered plugins (requires successful initialisation).
    pub async fn start_all(&mut self) -> ExtensionResult<()> {
        if !self.is_initialized {
            return Err(ExtensionError::NotInitialized);
        }

        info!("Starting {} plugins", self.plugins.len());

        for plugin_name in &self.plugin_order.clone() {
            if let Some(plugin) = self.plugins.get_mut(plugin_name) {
                info!("Starting plugin: {}", plugin_name);
                plugin.start().await.map_err(|err| {
                    error!("Failed to start plugin {}: {}", plugin_name, err);
                    err
                })?;
            }
        }

        info!("All plugins started successfully");
        Ok(())
    }

    /// Stop all plugins in reverse registration order.
    pub async fn stop_all(&mut self) -> ExtensionResult<()> {
        info!("Stopping {} plugins", self.plugins.len());

        for plugin_name in self.plugin_order.iter().rev() {
            if let Some(plugin) = self.plugins.get_mut(plugin_name) {
                info!("Stopping plugin: {}", plugin_name);
                if let Err(err) = plugin.stop().await {
                    warn!("Error stopping plugin {}: {}", plugin_name, err);
                }
            }
        }

        info!("All plugins stopped");
        Ok(())
    }

    /// Broadcast an event to every plugin.
    pub async fn broadcast_event(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        debug!("Broadcasting event: {:?}", event);

        for plugin_name in &self.plugin_order.clone() {
            if let Some(plugin) = self.plugins.get_mut(plugin_name) {
                if let Err(err) = plugin.handle_event(event).await {
                    warn!("Plugin {} failed to handle event: {}", plugin_name, err);
                }
            }
        }

        Ok(())
    }

    /// Retrieve an immutable plugin reference by name.
    pub fn get_plugin(&self, name: &str) -> Option<&dyn Plugin> {
        self.plugins.get(name).map(|p| p.as_ref())
    }

    /// Retrieve a mutable plugin reference by name.
    pub fn get_plugin_mut(&mut self, name: &str) -> Option<&mut Box<dyn Plugin>> {
        self.plugins.get_mut(name)
    }

    /// List plugin metadata in execution order.
    pub fn list_plugins(&self) -> Vec<&PluginInfo> {
        self.plugin_order
            .iter()
            .filter_map(|name| self.plugins.get(name).map(|p| p.info()))
            .collect()
    }

    /// Retrieve all plugin info entries matching the specified category.
    pub fn get_plugins_by_category(&self, category: &PluginCategory) -> Vec<&PluginInfo> {
        self.plugins
            .values()
            .map(|p| p.info())
            .filter(|info| &info.category == category)
            .collect()
    }

    /// Whether a plugin with the supplied name has been registered.
    pub fn has_plugin(&self, name: &str) -> bool {
        self.plugins.contains_key(name)
    }

    /// Total number of registered plugins.
    pub fn plugin_count(&self) -> usize {
        self.plugins.len()
    }
}

/// Helper macro for registering plugins via `inventory`.
#[macro_export]
macro_rules! register_plugin {
    ($plugin_type:ty) => {
        inventory::submit! {
            fn() -> Box<dyn $crate::plugin::Plugin> {
                Box::new(<$plugin_type>::new())
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tempfile::tempdir;

    struct TestPlugin {
        info: PluginInfo,
        initialized: AtomicBool,
        started: AtomicBool,
    }

    impl TestPlugin {
        fn new(name: &str, priority: i32) -> Self {
            Self {
                info: PluginInfo {
                    name: name.to_string(),
                    version: "1.0.0".to_string(),
                    description: "Test plugin".to_string(),
                    author: "Test Author".to_string(),
                    dependencies: vec![],
                    min_neo_version: "3.0.0".to_string(),
                    category: PluginCategory::Utility,
                    priority,
                },
                initialized: AtomicBool::new(false),
                started: AtomicBool::new(false),
            }
        }
    }

    #[async_trait]
    impl Plugin for TestPlugin {
        fn info(&self) -> &PluginInfo {
            &self.info
        }

        async fn initialize(&mut self, _context: &PluginContext) -> ExtensionResult<()> {
            self.initialized.store(true, Ordering::Relaxed);
            Ok(())
        }

        async fn start(&mut self) -> ExtensionResult<()> {
            self.started.store(true, Ordering::Relaxed);
            Ok(())
        }

        async fn stop(&mut self) -> ExtensionResult<()> {
            self.started.store(false, Ordering::Relaxed);
            Ok(())
        }

        async fn handle_event(&mut self, _event: &PluginEvent) -> ExtensionResult<()> {
            Ok(())
        }
    }

    fn create_test_context() -> PluginContext {
        let temp_dir = tempdir().expect("operation failed");
        PluginContext {
            neo_version: "3.6.0".to_string(),
            config_dir: temp_dir.path().to_path_buf(),
            data_dir: temp_dir.path().to_path_buf(),
            shared_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tokio::test]
    async fn test_plugin_manager() {
        let context = create_test_context();
        let mut manager = PluginManager::new(context);

        let plugin1 = Box::new(TestPlugin::new("plugin1", 10));
        let plugin2 = Box::new(TestPlugin::new("plugin2", 5));

        manager.register_plugin(plugin1).unwrap();
        manager.register_plugin(plugin2).unwrap();

        assert_eq!(manager.plugin_count(), 2);
        assert!(manager.has_plugin("plugin1"));
        assert!(manager.has_plugin("plugin2"));

        manager.initialize_all().await.unwrap();
        manager.start_all().await.unwrap();

        let event = PluginEvent::Custom {
            event_type: "test".to_string(),
            data: json!({"value": 1}),
        };
        manager.broadcast_event(&event).await.unwrap();

        manager.stop_all().await.unwrap();
    }

    #[test]
    fn test_plugin_info() {
        let info = PluginInfo {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test plugin".to_string(),
            author: "Test Author".to_string(),
            dependencies: vec!["dep1".to_string()],
            min_neo_version: "3.0.0".to_string(),
            category: PluginCategory::Core,
            priority: 100,
        };

        assert_eq!(info.name, "test");
        assert_eq!(info.category, PluginCategory::Core);
        assert_eq!(info.priority, 100);
    }
}
