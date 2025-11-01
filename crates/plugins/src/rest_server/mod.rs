//! RestServer plugin module
//!
//! This module provides the REST server plugin implementation matching the C# Neo.Plugins.RestServer exactly.

pub mod authentication;
pub mod binder;
pub mod controllers;
pub mod exceptions;
pub mod extensions;
pub mod helpers;
pub mod middleware;
pub mod models;
pub mod newtonsoft;
pub mod providers;
pub mod rest_server_plugin;
pub mod rest_server_settings;
pub mod rest_server_utility;
pub mod rest_server_utility_contract;
pub mod rest_server_utility_j_tokens;
pub mod rest_web_server;
pub mod tokens;

// Re-export commonly used types
pub use rest_server_plugin::{RestServerGlobals, RestServerPlugin};
pub use rest_server_settings::RestServerSettings;
pub use rest_server_utility::RestServerUtility;
#[allow(unused_imports)]
pub use rest_server_utility_contract::*;
pub use rest_web_server::RestWebServer;
