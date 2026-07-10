//! NEP transfer-history RPC handlers.
//!
//! Transfer handlers validate plugin capabilities and delegate indexed range
//! reads to the shared transfer collector. The root module only registers the
//! public JSON-RPC method names.

use crate::plugins::tokens_tracker::{Nep11Tracker, Nep17Tracker};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use serde_json::Value;

use super::RpcServerTokensTracker;
use super::helpers::{collect_nep11_transfers, collect_transfers, tracker_service};
use super::request::TransferHistoryRequest;
use super::response::transfer_history;

impl RpcServerTokensTracker {
    pub(super) fn get_nep11_transfers(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep11() || !service.settings().track_history {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = TransferHistoryRequest::parse(params, "getnep11transfers", address_version)?;

        let (_, sent_prefix, received_prefix) =
            Nep11Tracker::<neo_native_contracts::StandardNativeProvider>::rpc_prefixes();
        let max_results = service.settings().max_results_limit();

        let sent = collect_nep11_transfers(
            service.store().as_ref(),
            sent_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;
        let received = collect_nep11_transfers(
            service.store().as_ref(),
            received_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;

        Ok(transfer_history(
            &request.script_hash,
            address_version,
            sent,
            received,
        ))
    }

    pub(super) fn get_nep17_transfers(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let service = tracker_service(server)?;
        if !service.settings().enabled_nep17() || !service.settings().track_history {
            return Err(RpcException::from(RpcError::method_not_found()));
        }

        let address_version = server.system().settings().address_version;
        let request = TransferHistoryRequest::parse(params, "getnep17transfers", address_version)?;

        let (_, sent_prefix, received_prefix) =
            Nep17Tracker::<neo_native_contracts::StandardNativeProvider>::rpc_prefixes();
        let max_results = service.settings().max_results_limit();

        let sent = collect_transfers(
            service.store().as_ref(),
            sent_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;
        let received = collect_transfers(
            service.store().as_ref(),
            received_prefix,
            &request.script_hash,
            request.start,
            request.end,
            address_version,
            max_results,
        )?;

        Ok(transfer_history(
            &request.script_hash,
            address_version,
            sent,
            received,
        ))
    }
}
