use std::any::Any;

/// Trait for handling the ServiceAdded event from the NeoSystem.
pub trait IServiceAddedHandler {
    /// Handler for the ServiceAdded event from the NeoSystem.
    ///
    /// This function is triggered when a service is added to the NeoSystem.
    ///
    /// # Arguments
    ///
    /// * `sender` - A reference to the source of the event.
    /// * `service` - A reference to the service that was added.
    fn neo_system_service_added_handler(&self, sender: &dyn Any, service: &dyn Any);
}
