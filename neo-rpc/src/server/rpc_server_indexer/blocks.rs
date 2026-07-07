//! Block-index RPC endpoint handlers.

use serde_json::Value;

use super::{BlockSelector, RpcServerIndexer, STANDARD_PAGE_BOUNDS};
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerIndexer {
    pub(super) fn get_block_index(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        Self::expect_exact_params(params, 1, "getblockindex")?;
        let service = Self::service(server)?;
        let record = match Self::parse_block_selector(params, "getblockindex")? {
            BlockSelector::Height(height) => service
                .try_block_by_height(height)
                .map_err(Self::indexer_error)?,
            BlockSelector::Hash(hash) => service
                .try_block_by_hash(&hash)
                .map_err(Self::indexer_error)?,
        };
        Ok(record.map_or(Value::Null, Self::block_to_json))
    }

    pub(super) fn get_block_indexes(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = Self::service(server)?;
        let (skip, limit) = Self::parse_page(params, 0, STANDARD_PAGE_BOUNDS, "getblockindexes")?;

        Ok(Value::Array(
            service
                .try_blocks(skip, limit)
                .map_err(Self::indexer_error)?
                .into_iter()
                .map(Self::block_to_json)
                .collect(),
        ))
    }
}
