//! Shared support types and service helpers for indexer RPC handlers.

use std::sync::Arc;

use neo_indexer::{IndexerError, IndexerService};
use neo_primitives::UInt256;

use super::RpcServerIndexer;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_server::RpcServer;

pub(super) const STANDARD_PAGE_BOUNDS: PageBounds = PageBounds {
    default_limit: 100,
    max_limit: 1_000,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct PageBounds {
    pub(super) default_limit: usize,
    pub(super) max_limit: usize,
}

pub(super) enum BlockSelector {
    Height(u32),
    Hash(UInt256),
}

impl RpcServerIndexer {
    pub(super) fn service(server: &RpcServer) -> Result<Arc<IndexerService>, RpcException> {
        server
            .system()
            .indexer_service()
            .ok_or_else(|| internal_error("NeoIndexer service not available"))
    }

    pub(super) fn indexer_error(error: IndexerError) -> RpcException {
        internal_error(format!("NeoIndexer service read failed: {error}"))
    }
}
