//! Service registration and handler wiring for `NeoSystem`.
//!
//! These methods are kept in a dedicated module to keep `core.rs` focused on
//! construction and orchestration.

use std::any::Any;
use std::sync::Arc;

use tracing::warn;

use super::context::NeoSystemContext;
use super::helpers::block_on_extension;
use super::NeoSystem;
use crate::error::CoreResult;
use crate::extensions::plugin::{broadcast_global_event, PluginEvent};
use crate::i_event_handlers::{
    ICommittedHandler, ICommittingHandler, ILogHandler, ILoggingHandler, INotifyHandler,
    IServiceAddedHandler, ITransactionAddedHandler, ITransactionRemovedHandler,
    IWalletChangedHandler,
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
        handler: Arc<dyn ICommittingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_committing_handler(handler)
    }

    /// Registers a handler invoked after block commit completes.
    pub fn register_committed_handler(
        &self,
        handler: Arc<dyn ICommittedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_committed_handler(handler)
    }

    /// Registers a handler invoked when transactions enter the memory pool.
    pub fn register_transaction_added_handler(
        &self,
        handler: Arc<dyn ITransactionAddedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_transaction_added_handler(handler)
    }

    /// Registers a handler invoked when transactions leave the memory pool.
    pub fn register_transaction_removed_handler(
        &self,
        handler: Arc<dyn ITransactionRemovedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_transaction_removed_handler(handler)
    }

    /// Registers a handler for `ApplicationEngine.Log` events.
    pub fn register_log_handler(
        &self,
        handler: Arc<dyn ILogHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_log_handler(handler)
    }

    /// Registers a handler for `Utility.Logging` events.
    pub fn register_logging_handler(
        &self,
        handler: Arc<dyn ILoggingHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_logging_handler(handler)
    }

    /// Registers a handler for `ApplicationEngine.Notify` events.
    pub fn register_notify_handler(
        &self,
        handler: Arc<dyn INotifyHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_notify_handler(handler)
    }

    /// Registers a handler for wallet provider changes.
    pub fn register_wallet_changed_handler(
        &self,
        handler: Arc<dyn IWalletChangedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        self.context().register_wallet_changed_handler(handler)
    }

    /// Attaches a wallet provider so wallet-change notifications propagate to handlers.
    pub fn attach_wallet_provider(
        &self,
        provider: Arc<dyn crate::wallets::IWalletProvider + Send + Sync>,
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

    fn notify_service_added(&self, service: Arc<dyn Any + Send + Sync>, name: Option<String>) {
        let sender: &dyn Any = self;
        if let Ok(handlers) = self.context().service_added_handlers.read() {
            for handler in handlers.iter() {
                handler.neo_system_service_added_handler(sender, service.as_ref());
            }
        }

        if let Ok(guard) = self.self_ref.lock() {
            if let Some(system) = guard.clone().upgrade() {
                // Note: Cannot use Arc::clone() here because we need to coerce Arc<NeoSystem> to Arc<dyn Any>
                let system_any: Arc<dyn Any + Send + Sync> = system.clone();
                let event = PluginEvent::ServiceAdded {
                    system: system_any,
                    name,
                    service,
                };
                if let Err(err) = block_on_extension(broadcast_global_event(&event)) {
                    warn!("failed to broadcast ServiceAdded event: {}", err);
                }
            }
        }
    }

    /// Registers a handler to be notified when services are added.
    pub fn register_service_added_handler(
        &self,
        handler: Arc<dyn IServiceAddedHandler + Send + Sync>,
    ) -> CoreResult<()> {
        let context = self.context();
        let mut guard = context
            .service_added_handlers
            .write()
            .map_err(|_| crate::error::CoreError::system("service handler registry poisoned"))?;
        guard.push(handler);
        Ok(())
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
