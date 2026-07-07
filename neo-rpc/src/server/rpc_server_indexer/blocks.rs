//! Block-index RPC endpoint handlers.

use serde_json::Value;

use super::params::{BlockIndexRequest, PageRequest};
use super::{BlockSelector, RpcServerIndexer, STANDARD_PAGE_BOUNDS};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerIndexer {
    pub(super) fn get_block_index(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = BlockIndexRequest::parse(params)?;
        let service = Self::service(server)?;
        let record = match request.selector {
            BlockSelector::Height(height) => service
                .try_block_by_height(height)
                .map_err(Self::indexer_error)?,
            BlockSelector::Hash(hash) => service
                .try_block_by_hash(&hash)
                .map_err(Self::indexer_error)?,
        };
        Ok(Self::optional_block_to_json(record))
    }

    pub(super) fn get_block_indexes(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let request = PageRequest::parse(params, 0, STANDARD_PAGE_BOUNDS, "getblockindexes")?;

        let records = service
            .try_blocks(request.skip, request.limit)
            .map_err(Self::indexer_error)?;
        Ok(Self::blocks_to_json(records))
    }
}
