//! Plugin system for Neo extensions, mirroring the behaviour of
//! `Neo.Extensions.Plugin` in the C# codebase.

use crate::error::{ExtensionError, ExtensionResult};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock as AsyncRwLock;
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

/// Behaviour when plugins throw unhandled exceptions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnhandledExceptionPolicy {
    Ignore,
    StopPlugin,
    StopNode,
}

impl Default for UnhandledExceptionPolicy {
    fn default() -> Self {
        UnhandledExceptionPolicy::StopNode
    }
}

/// Returns the base directory where plugins reside (equivalent to C# `PluginsDirectory`).
pub fn plugins_directory() -> PathBuf {
    application_root()
        .map(|root| root.join("Plugins"))
        .unwrap_or_else(|| PathBuf::from("Plugins"))
}

fn application_root() -> Option<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// Shared base implementation mirroring the C# `Plugin` abstract class.
#[derive(Debug)]
pub struct PluginBase {
    info: PluginInfo,
    exception_policy: UnhandledExceptionPolicy,
    root_path: PathBuf,
    config_file: PathBuf,
    is_stopped: AtomicBool,
}

impl PluginBase {
    /// Creates a new plugin base instance with the supplied metadata.
    pub fn new(info: PluginInfo) -> Self {
        let root_path = plugins_directory().join(&info.name);
        let config_file = root_path.join("config.json");
        let base = Self {
            info,
            exception_policy: UnhandledExceptionPolicy::default(),
            root_path,
            config_file,
            is_stopped: AtomicBool::new(false),
        };
        // Ensure the plugin root path exists so configuration can be stored.
        if let Err(err) = base.ensure_directories() {
            warn!("failed to ensure plugin directory: {}", err);
        }
        base
    }

    /// Overrides the default exception policy.
    pub fn with_exception_policy(mut self, policy: UnhandledExceptionPolicy) -> Self {
        self.exception_policy = policy;
        self
    }

    /// Plugin metadata accessor.
    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    /// Plugin root directory.
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    /// Plugin configuration file path.
    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    /// Returns the configured exception policy for the plugin.
    pub fn exception_policy(&self) -> UnhandledExceptionPolicy {
        self.exception_policy
    }

    /// Marks the plugin as stopped following an unrecoverable error.
    pub fn mark_stopped(&self) {
        self.is_stopped.store(true, Ordering::Relaxed);
    }

    /// Indicates whether the plugin has been stopped due to an error.
    pub fn is_stopped(&self) -> bool {
        self.is_stopped.load(Ordering::Relaxed)
    }

    /// Ensures the plugin root directory exists.
    pub fn ensure_directories(&self) -> std::io::Result<()> {
        if !self.root_path.exists() {
            fs::create_dir_all(&self.root_path)?;
        }
        Ok(())
    }

    /// Loads the plugin configuration file if present.
    pub fn load_configuration(&self) -> ExtensionResult<Option<serde_json::Value>> {
        if !self.config_file.exists() {
            return Ok(None);
        }

        let mut file = File::open(&self.config_file)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        if contents.trim().is_empty() {
            return Ok(None);
        }

        serde_json::from_str(&contents)
            .map(Some)
            .map_err(|err| ExtensionError::invalid_config(err.to_string()))
    }
}

/// Context supplied to plugins during initialisation.
#[derive(Debug, Clone)]
pub struct PluginContext {
    pub neo_version: String,
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub shared_data: Arc<AsyncRwLock<HashMap<String, serde_json::Value>>>,
}

impl PluginContext {
    /// Creates a new plugin context with the provided directories.
    pub fn new(neo_version: impl Into<String>, config_dir: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            neo_version: neo_version.into(),
            config_dir,
            data_dir,
            shared_data: Arc::new(AsyncRwLock::new(HashMap::new())),
        }
    }

    /// Builds a plugin context using the standard plugins directory.
    pub fn from_environment() -> Self {
        let plugins_dir = plugins_directory();
        Self::new(env!("CARGO_PKG_VERSION"), plugins_dir.clone(), plugins_dir)
    }
}

/// Events that can be broadcast to plugins.
#[derive(Clone)]
pub enum PluginEvent {
    /// Equivalent to `Plugin.OnSystemLoaded` in C#, providing the node instance as a dynamic reference.
    NodeStarted {
        system: Arc<dyn std::any::Any + Send + Sync>,
    },
    /// Node stopping notification.
    NodeStopping,
    /// Service registered via `NeoSystem::add_service`.
    ServiceAdded {
        system: Arc<dyn std::any::Any + Send + Sync>,
        name: Option<String>,
        service: Arc<dyn std::any::Any + Send + Sync>,
    },
    /// New block received event.
    BlockReceived {
        block_hash: String,
        block_height: u32,
    },
    /// New transaction received event (validated and relayed).
    TransactionReceived { tx_hash: String },
    /// Transaction accepted into the mempool.
    MempoolTransactionAdded { tx_hash: String },
    /// Transactions removed from the mempool along with the reason.
    MempoolTransactionRemoved {
        tx_hashes: Vec<String>,
        reason: String,
    },
    /// Wallet was opened or closed.
    WalletChanged { wallet_name: String },
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

impl std::fmt::Debug for PluginEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginEvent::NodeStarted { .. } => f.write_str("PluginEvent::NodeStarted"),
            PluginEvent::NodeStopping => f.write_str("PluginEvent::NodeStopping"),
            PluginEvent::ServiceAdded { name, .. } => {
                if let Some(name) = name {
                    f.debug_struct("PluginEvent::ServiceAdded")
                        .field("name", name)
                        .finish()
                } else {
                    f.write_str("PluginEvent::ServiceAdded")
                }
            }
            PluginEvent::BlockReceived {
                block_hash,
                block_height,
            } => f
                .debug_struct("PluginEvent::BlockReceived")
                .field("block_hash", block_hash)
                .field("block_height", block_height)
                .finish(),
            PluginEvent::TransactionReceived { tx_hash } => f
                .debug_struct("PluginEvent::TransactionReceived")
                .field("tx_hash", tx_hash)
                .finish(),
            PluginEvent::ConsensusStateChanged { state } => f
                .debug_struct("PluginEvent::ConsensusStateChanged")
                .field("state", state)
                .finish(),
            PluginEvent::MempoolTransactionAdded { tx_hash } => f
                .debug_struct("PluginEvent::MempoolTransactionAdded")
                .field("tx_hash", tx_hash)
                .finish(),
            PluginEvent::MempoolTransactionRemoved { tx_hashes, reason } => f
                .debug_struct("PluginEvent::MempoolTransactionRemoved")
                .field("tx_hashes", tx_hashes)
                .field("reason", reason)
                .finish(),
            PluginEvent::WalletChanged { wallet_name } => f
                .debug_struct("PluginEvent::WalletChanged")
                .field("wallet_name", wallet_name)
                .finish(),
            PluginEvent::RpcRequest { method, params } => f
                .debug_struct("PluginEvent::RpcRequest")
                .field("method", method)
                .field("params", params)
                .finish(),
            PluginEvent::Custom { event_type, data } => f
                .debug_struct("PluginEvent::Custom")
                .field("event_type", event_type)
                .field("data", data)
                .finish(),
        }
    }
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

    /// Returns the plugin execution context.
    pub fn context(&self) -> &PluginContext {
        &self.context
    }
}

/// Registration wrapper used with the `inventory` crate for plugin discovery.
pub struct PluginRegistration(pub fn() -> Box<dyn Plugin>);

inventory::collect!(PluginRegistration);

/// Runtime responsible for loading, initialising, and broadcasting events to plugins.
pub struct PluginRuntime {
    manager: PluginManager,
    initialized: bool,
}

impl PluginRuntime {
    /// Creates a new runtime using the supplied context.
    pub fn new(context: PluginContext) -> Self {
        Self {
            manager: PluginManager::new(context),
            initialized: false,
        }
    }

    /// Creates a new runtime using the default plugins directory context.
    pub fn from_environment() -> Self {
        Self::new(PluginContext::from_environment())
    }

    /// Initialises and starts all registered plugins.
    pub async fn initialize(&mut self) -> ExtensionResult<()> {
        if !self.initialized {
            self.register_inventory_plugins()?;
            self.manager.initialize_all().await?;
            self.manager.start_all().await?;
            self.initialized = true;
        }
        Ok(())
    }

    /// Stops all plugins and resets the runtime state.
    pub async fn shutdown(&mut self) -> ExtensionResult<()> {
        if self.initialized {
            self.manager.stop_all().await?;
            self.initialized = false;
        }
        Ok(())
    }

    /// Broadcasts an event to all registered plugins.
    pub async fn broadcast(&mut self, event: &PluginEvent) -> ExtensionResult<()> {
        self.manager.broadcast_event(event).await
    }

    fn register_inventory_plugins(&mut self) -> ExtensionResult<()> {
        for registration in inventory::iter::<PluginRegistration> {
            let factory = registration.0;
            let plugin = factory();
            let name = plugin.info().name.clone();
            self.manager.register_plugin(plugin).map_err(|err| {
                error!("failed to register plugin '{}': {}", name, err);
                err
            })?;
        }
        Ok(())
    }

    /// Access to the underlying manager.
    pub fn manager(&self) -> &PluginManager {
        &self.manager
    }

    /// Mutable access to the underlying manager.
    pub fn manager_mut(&mut self) -> &mut PluginManager {
        &mut self.manager
    }
}

static GLOBAL_RUNTIME: Lazy<AsyncRwLock<Option<PluginRuntime>>> =
    Lazy::new(|| AsyncRwLock::new(None));

/// Ensures a global plugin runtime is initialised with the provided context.
pub async fn initialise_global_runtime(context: Option<PluginContext>) -> ExtensionResult<()> {
    let mut runtime = match context {
        Some(ctx) => PluginRuntime::new(ctx),
        None => PluginRuntime::from_environment(),
    };
    runtime.initialize().await?;
    let mut guard = GLOBAL_RUNTIME.write().await;
    *guard = Some(runtime);
    Ok(())
}

/// Shuts down and clears the global plugin runtime if initialised.
pub async fn shutdown_global_runtime() -> ExtensionResult<()> {
    let mut guard = GLOBAL_RUNTIME.write().await;
    if let Some(runtime) = guard.as_mut() {
        runtime.shutdown().await?;
    }
    *guard = None;
    Ok(())
}

/// Broadcasts an event to all plugins through the global runtime, if initialised.
pub async fn broadcast_global_event(event: &PluginEvent) -> ExtensionResult<()> {
    let mut guard = GLOBAL_RUNTIME.write().await;
    if let Some(runtime) = guard.as_mut() {
        runtime.broadcast(event).await?;
    }
    Ok(())
}

/// Helper macro for registering plugins via `inventory`.
#[macro_export]
macro_rules! register_plugin {
    ($plugin_type:ty) => {
        inventory::submit! {
            $crate::plugin::PluginRegistration(|| {
                Box::new(<$plugin_type>::new())
            })
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
        PluginContext::new(
            "3.6.0",
            temp_dir.path().to_path_buf(),
            temp_dir.path().to_path_buf(),
        )
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
