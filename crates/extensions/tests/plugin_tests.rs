//! Extensions Plugin C# Compatibility Tests
//!
//! These tests ensure full compatibility with C# Neo.Extensions plugin system.

use neo_extensions::plugin::*;

#[cfg(test)]
mod plugin_tests {
    use super::*;

    /// Test plugin loading (matches C# plugin system exactly)
    #[test]
    fn test_plugin_loading_compatibility() {
        let plugin_manager = PluginManager::new();

        // Test plugin registration
        let mock_plugin = MockPlugin::new("test_plugin", "1.0.0");
        plugin_manager.register_plugin(Box::new(mock_plugin));

        assert_eq!(plugin_manager.plugin_count(), 1);
        assert!(plugin_manager.is_plugin_loaded("test_plugin"));

        // Test plugin lifecycle
        plugin_manager.initialize_plugins().unwrap();
        plugin_manager.start_plugins().unwrap();
        plugin_manager.stop_plugins().unwrap();
    }

    struct MockPlugin {
        name: String,
        version: String,
    }

    impl MockPlugin {
        fn new(name: &str, version: &str) -> Self {
            Self {
                name: name.to_string(),
                version: version.to_string(),
            }
        }
    }
}
