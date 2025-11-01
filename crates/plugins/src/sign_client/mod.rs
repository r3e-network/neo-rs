//! SignClient plugin module
//!
//! This module provides the sign client plugin implementation matching the C# Neo.Plugins.SignClient exactly.

pub mod settings;
pub mod sign_client;
pub mod sign_client_impl;
pub mod vsock;

// Re-export commonly used types
pub use settings::SignSettings;
pub use sign_client::SignClient;
pub use vsock::Vsock;
