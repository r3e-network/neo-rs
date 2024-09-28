use std::any::Any;
use crate::neo_contract::log_event_args::LogEventArgs;

/// The `LogHandlerTrait` trait defines the interface for handling Log events
/// from the ApplicationEngine in the NEO blockchain.
pub trait LogHandlerTrait {
    /// Handles the Log event from the ApplicationEngine.
    ///
    /// This method is triggered when a contract calls System.Runtime.Log.
    ///
    /// # Arguments
    ///
    /// * `sender` - A reference to the source of the event.
    /// * `log_event_args` - The arguments of the log event.
    fn handle_application_engine_log(&self, sender: &dyn Any, log_event_args: &LogEventArgs);
}
