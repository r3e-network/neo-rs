//! Internal helper functions for the neo_system module.
//!
//! This module provides utility functions used internally by the NeoSystem:
//!
//! - `block_on_extension` - Execute async futures in blocking contexts
//! - `initialise_plugins` - Bootstrap the plugin system
//! - `to_core_error` - Convert Akka errors to CoreError

use std::future::Future;

use tokio::runtime::{Builder as RuntimeBuilder, Handle as RuntimeHandle, RuntimeFlavor};
use tokio::task::block_in_place;

use crate::error::{CoreError, CoreResult};
use crate::extensions::error::ExtensionError;
use crate::extensions::plugin::{
    broadcast_global_event, initialise_global_runtime, PluginContext, PluginEvent,
};
use std::any::Any;
use std::sync::Arc;

use super::NeoSystem;

/// Executes an async future that returns an `ExtensionError` in a blocking context.
///
/// This helper handles the complexity of running async code from synchronous contexts,
/// properly detecting the current runtime flavor and using the appropriate blocking strategy.
pub(crate) fn block_on_extension<F, T>(future: F) -> Result<T, ExtensionError>
where
    F: Future<Output = Result<T, ExtensionError>> + Send,
    T: Send,
{
    if let Ok(handle) = RuntimeHandle::try_current() {
        match handle.runtime_flavor() {
            RuntimeFlavor::MultiThread => block_in_place(|| handle.block_on(future)),
            RuntimeFlavor::CurrentThread => RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| ExtensionError::operation_failed(err.to_string()))?
                .block_on(future),
            _ => RuntimeBuilder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| ExtensionError::operation_failed(err.to_string()))?
                .block_on(future),
        }
    } else {
        RuntimeBuilder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| ExtensionError::operation_failed(err.to_string()))?
            .block_on(future)
    }
}

/// Initializes the global plugin runtime and broadcasts the NodeStarted event.
///
/// This function sets up the plugin infrastructure and notifies all registered
/// plugins that the node has started.
pub(crate) fn initialise_plugins(system: &Arc<NeoSystem>) -> CoreResult<()> {
    let context = PluginContext::from_environment();
    block_on_extension(initialise_global_runtime(Some(context))).map_err(|err| {
        CoreError::system(format!("failed to initialize plugin runtime: {}", err))
    })?;

    // Auto-register core plugins based on compiled feature set to mirror C# defaults.
    // Note: Cannot use Arc::clone() here because we need to coerce Arc<NeoSystem> to Arc<dyn Any>
    let system_any: Arc<dyn Any + Send + Sync> = system.clone();
    let event = PluginEvent::NodeStarted { system: system_any };
    block_on_extension(broadcast_global_event(&event))
        .map_err(|err| CoreError::system(format!("failed to broadcast NodeStarted: {}", err)))?;
    Ok(())
}

/// Converts an Akka actor system error into a CoreError.
pub(crate) fn to_core_error(err: akka::AkkaError) -> CoreError {
    CoreError::system(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_core_error_preserves_message() {
        let akka_err = akka::AkkaError::actor("test_actor not found");
        let core_err = to_core_error(akka_err);
        let msg = format!("{}", core_err);
        assert!(msg.contains("test_actor"));
    }
}
