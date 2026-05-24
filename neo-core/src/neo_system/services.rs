//! Service registration and handler wiring for `NeoSystem`.
//!
//! These methods are kept in a dedicated module to keep `core.rs` focused on
//! construction and orchestration.

use std::any::Any;
use std::sync::Arc;

use super::context::NeoSystemContext;
use super::NeoSystem;
use crate::error::CoreResult;
use crate::events::{broadcast_plugin_event, PluginEvent};
use crate::i_event_handlers::{
    CommittedHandler, CommittingHandler, WalletChangedHandler,
};

impl NeoSystem {
    /// Registers an arbitrary service instance for later retrieval (parity with C# `NeoSystem.AddService`).
    pub fn add_service<T, S>(&self, service: S) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
        S: Into<Arc<T>>,
    {
        self.add_service_internal::<T>(None, service.into())
    }

    /// Registers an arbitrary service instance with an explicit name (compatibility helper).
    pub fn add_named_service<T, S>(&self, name: impl Into<String>, service: S) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
        S: Into<Arc<T>>,
    {
        self.add_service_internal::<T>(Some(name.into()), service.into())
    }

    /// Registers a handler invoked before block commit.
    pub fn register_committing_handler(
        &self,
        handler: Arc<dyn CommittingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_committing_handler(handler)
    }

    /// Registers a handler invoked after block commit completes.
    pub fn register_committed_handler(
        &self,
        handler: Arc<dyn CommittedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_committed_handler(handler)
    }

    /// Registers a handler for wallet provider changes.
    pub fn register_wallet_changed_handler(
        &self,
        handler: Arc<dyn WalletChangedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_wallet_changed_handler(handler)
    }

    /// Attaches a wallet provider so wallet-change notifications propagate to handlers.
    pub fn attach_wallet_provider(
        &self,
        provider: Arc<dyn crate::wallets::WalletProvider + Send + Sync>,
    ) -> CoreResult<()> {
        NeoSystemContext::attach_wallet_provider(&self.context(), provider)
    }

    fn add_service_internal<T>(&self, name: Option<String>, service: Arc<T>) -> CoreResult<()>
    where
        T: Any + Send + Sync + 'static,
    {
        // Note: Cannot use Arc::clone() here because we need to coerce Arc<T> to Arc<dyn Any>
        let service_any: Arc<dyn Any + Send + Sync> = service.clone();

        // leverage the registry stored on the context for consistency
        self.context()
            .service_registry
            .register(Arc::clone(&service_any), name.clone())?;

        self.notify_service_added(service_any, name);
        Ok(())
    }

    fn notify_service_added(&self, _service: Arc<dyn Any + Send + Sync>, name: Option<String>) {
        let system = { self.self_ref.lock().clone().upgrade() };
        if system.is_some() {
            let event = PluginEvent::ServiceAdded {
                service_name: name.unwrap_or_else(|| "unnamed".to_string()),
            };
            broadcast_plugin_event(&event);
        }
    }

    /// Returns the RPC service name for the current network (if configured).
    pub fn rpc_service_name(&self) -> String {
        self.context().rpc_service_name()
    }

    /// Retrieves the first registered service assignable to `T`.
    pub fn get_service<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.context().get_service::<T>()
    }

    /// Returns `true` if a named service is registered.
    pub fn has_named_service(&self, name: &str) -> bool {
        self.context().has_named_service(name)
    }

    /// Returns the RPC service registered for this network (if available).
    pub fn rpc_service<T>(&self) -> CoreResult<Option<Arc<T>>>
    where
        T: Any + Send + Sync + 'static,
    {
        self.context().rpc_service()
    }
}
