//! Helpers for accessing shared services registered on the `NeoSystem`.
use crate::rpc_server::rpc_server::RpcServer;
use neo_core::ledger::LedgerContext;
use neo_core::neo_system::{NeoSystem, NeoSystemContext};
use neo_core::network::p2p::local_node::LocalNode;
use neo_core::services::{LedgerService, MempoolService, PeerManagerService, RpcService};
use neo_core::CoreResult;
use parking_lot::RwLock;
use std::sync::{Arc, Mutex};

/// Typed accessor for retrieving the RPC server service registration.
pub trait RpcServiceAccess {
    /// Returns the RPC server service bound to the current network (if registered).
    fn rpc_server(&self) -> CoreResult<Option<Arc<RwLock<RpcServer>>>>;

    /// Returns the RPC server as the typed readiness trait.
    fn rpc_server_typed(&self) -> CoreResult<Option<Arc<dyn RpcService + Send + Sync>>> {
        Ok(self
            .rpc_server()?
            .map(|srv| Arc::new(RpcServerHandle::new(srv)) as Arc<dyn RpcService + Send + Sync>))
    }
}

impl RpcServiceAccess for NeoSystemContext {
    fn rpc_server(&self) -> CoreResult<Option<Arc<RwLock<RpcServer>>>> {
        self.rpc_service::<RwLock<RpcServer>>()
    }
}

impl RpcServiceAccess for NeoSystem {
    fn rpc_server(&self) -> CoreResult<Option<Arc<RwLock<RpcServer>>>> {
        self.context().rpc_server()
    }
}

/// Typed accessor for retrieving shared core services from the registry.
pub trait CoreServiceAccess {
    /// Returns the ledger service instance if registered.
    fn ledger_service(&self) -> CoreResult<Option<Arc<LedgerContext>>>;
    /// Returns the shared memory pool service if registered.
    fn mempool_service(&self) -> CoreResult<Option<Arc<Mutex<neo_core::ledger::MemoryPool>>>>;
    /// Returns the local node service if registered.
    fn local_node_service(&self) -> CoreResult<Option<Arc<LocalNode>>>;
    /// Returns the ledger service as the typed trait.
    fn ledger_typed(&self) -> CoreResult<Option<Arc<dyn LedgerService>>>;
    /// Returns the mempool service as the typed trait.
    fn mempool_typed(&self) -> CoreResult<Option<Arc<dyn MempoolService + Send + Sync>>>;
    /// Returns the peer manager service as the typed trait.
    fn peer_manager_typed(&self) -> CoreResult<Option<Arc<dyn PeerManagerService>>>;
}

impl CoreServiceAccess for NeoSystemContext {
    fn ledger_service(&self) -> CoreResult<Option<Arc<LedgerContext>>> {
        self.ledger_service()
    }

    fn mempool_service(&self) -> CoreResult<Option<Arc<Mutex<neo_core::ledger::MemoryPool>>>> {
        self.mempool_service()
    }

    fn local_node_service(&self) -> CoreResult<Option<Arc<LocalNode>>> {
        self.local_node_service()
    }

    fn ledger_typed(&self) -> CoreResult<Option<Arc<dyn LedgerService>>> {
        Ok(self
            .ledger_service()?
            .map(|svc| svc as Arc<dyn LedgerService>))
    }

    fn mempool_typed(&self) -> CoreResult<Option<Arc<dyn MempoolService + Send + Sync>>> {
        NeoSystemContext::mempool_typed(self)
    }

    fn peer_manager_typed(&self) -> CoreResult<Option<Arc<dyn PeerManagerService>>> {
        Ok(self
            .local_node_service()?
            .map(|svc| svc as Arc<dyn PeerManagerService>))
    }
}

impl CoreServiceAccess for NeoSystem {
    fn ledger_service(&self) -> CoreResult<Option<Arc<LedgerContext>>> {
        self.context().ledger_service()
    }

    fn mempool_service(&self) -> CoreResult<Option<Arc<Mutex<neo_core::ledger::MemoryPool>>>> {
        self.context().mempool_service()
    }

    fn local_node_service(&self) -> CoreResult<Option<Arc<LocalNode>>> {
        self.context().local_node_service()
    }

    fn ledger_typed(&self) -> CoreResult<Option<Arc<dyn LedgerService>>> {
        self.context().ledger_typed()
    }

    fn mempool_typed(&self) -> CoreResult<Option<Arc<dyn MempoolService + Send + Sync>>> {
        self.context().mempool_typed()
    }

    fn peer_manager_typed(&self) -> CoreResult<Option<Arc<dyn PeerManagerService>>> {
        self.context().peer_manager_typed()
    }
}

/// Thin wrapper that exposes an `Arc<RwLock<RpcServer>>` through the `RpcService` trait.
struct RpcServerHandle {
    inner: Arc<RwLock<RpcServer>>,
}

impl RpcServerHandle {
    fn new(inner: Arc<RwLock<RpcServer>>) -> Self {
        Self { inner }
    }
}

impl RpcService for RpcServerHandle {
    fn is_started(&self) -> bool {
        self.inner.read().is_started()
    }
}
