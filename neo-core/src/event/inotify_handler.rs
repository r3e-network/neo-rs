use std::any::Any;
use crate::neo_contract::notify_event_args::NotifyEventArgs;

/// Trait for handling Notify events from the ApplicationEngine.
pub trait INotifyHandler {
    /// Handler for the Notify event from the ApplicationEngine.
    ///
    /// This method is triggered when a contract calls System.Runtime.Notify.
    ///
    /// # Arguments
    ///
    /// * `sender` - A reference to the source of the event.
    /// * `notify_event_args` - The arguments of the notification.
    fn handle_application_engine_notify(&self, sender: &dyn Any, notify_event_args: &NotifyEventArgs);
}
