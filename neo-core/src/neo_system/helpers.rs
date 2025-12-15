//! Internal helper functions for the neo_system module.
//!
//! This module provides utility functions used internally by the NeoSystem:
//!
//! - `initialise_plugins` - Bootstrap event logging (plugin system removed)
//! - `to_core_error` - Convert Akka errors to CoreError

use std::sync::Arc;

use tracing::info;

use crate::error::{CoreError, CoreResult};
use crate::events::{broadcast_plugin_event, PluginEvent};

use super::NeoSystem;

/// Initializes event logging for the node startup.
///
/// This function broadcasts the NodeStarted event. The plugin system
/// has been removed and replaced with simple event logging.
#[allow(unused_variables)]
pub(crate) fn initialise_plugins(system: &Arc<NeoSystem>) -> CoreResult<()> {
    info!(target: "neo", "Node starting, broadcasting NodeStarted event");
    broadcast_plugin_event(&PluginEvent::NodeStarted {
        system: Arc::clone(system) as Arc<dyn std::any::Any + Send + Sync>,
    });
    Ok(())
}

/// Converts an Akka actor system error into a CoreError.
pub(crate) fn to_core_error(err: crate::akka::AkkaError) -> CoreError {
    CoreError::system(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_core_error_preserves_message() {
        let akka_err = crate::akka::AkkaError::actor("test_actor not found");
        let core_err = to_core_error(akka_err);
        let msg = format!("{}", core_err);
        assert!(msg.contains("test_actor"));
    }
}
