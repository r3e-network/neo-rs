use crate::server::rpc_server::{RpcHandler, RpcServer};
use crate::server::rpc_server_application_logs::RpcServerApplicationLogs;
use crate::server::rpc_server_indexer::RpcServerIndexer;
use crate::server::rpc_server_oracle::RpcServerOracle;
use crate::server::rpc_server_state::RpcServerState;
use crate::server::rpc_server_tokens_tracker::RpcServerTokensTracker;
use crate::server::rpc_server_utilities::RpcServerUtilities;
use crate::server::rpc_server_utilities::response::{service_entry_to_json, services_to_json};
use serde_json::{Value, json};

impl RpcServer {
    /// List runtime services and their RPC method groups for node operators.
    pub fn list_services(&self) -> Value {
        let persistence_interfaces = ["IPersistencePlugin"];
        services_to_json(vec![
            service_entry_to_json(
                "RpcServer",
                &[],
                handler_names(RpcServerUtilities::register_handlers()),
                true,
                true,
                json!({
                    "transportmethods": self.transport_method_names().len(),
                    "authconfigured": self.rpc_auth_configured(),
                }),
            ),
            self.state_service_entry(&persistence_interfaces),
            self.application_logs_entry(&persistence_interfaces),
            self.tokens_tracker_entry(&persistence_interfaces),
            self.indexer_entry(&persistence_interfaces),
            self.oracle_entry(),
        ])
    }

    fn state_service_entry(&self, interfaces: &[&str]) -> Value {
        let state_store = self.system().state_store();
        service_entry_to_json(
            "StateService",
            interfaces,
            handler_names(RpcServerState::register_handlers()),
            state_store.is_some(),
            state_store.is_some(),
            state_store.map_or(Value::Null, |store| {
                json!({
                    "localrootindex": store.current_local_index(),
                })
            }),
        )
    }

    fn application_logs_entry(&self, interfaces: &[&str]) -> Value {
        let application_logs = self.system().application_logs_service();
        service_entry_to_json(
            "ApplicationLogs",
            interfaces,
            handler_names(RpcServerApplicationLogs::register_handlers()),
            application_logs.is_some(),
            application_logs.is_some(),
            application_logs.map_or(Value::Null, |logs| {
                let settings = logs.settings();
                json!({
                    "path": settings.path,
                    "debug": settings.debug,
                    "maxstacksize": settings.max_stack_size,
                    "exceptionpolicy": format!("{:?}", settings.exception_policy),
                })
            }),
        )
    }

    fn tokens_tracker_entry(&self, interfaces: &[&str]) -> Value {
        let tokens_tracker = self.system().tokens_tracker_service();
        service_entry_to_json(
            "TokensTracker",
            interfaces,
            handler_names(RpcServerTokensTracker::register_handlers()),
            tokens_tracker.is_some(),
            tokens_tracker.is_some(),
            tokens_tracker.map_or(Value::Null, |tracker| {
                let settings = tracker.settings();
                json!({
                    "dbpath": settings.db_path,
                    "trackhistory": settings.track_history,
                    "maxresults": settings.max_results,
                    "enabledtrackers": settings.enabled_trackers,
                    "exceptionpolicy": format!("{:?}", settings.exception_policy),
                })
            }),
        )
    }

    fn indexer_entry(&self, interfaces: &[&str]) -> Value {
        let indexer = self.system().indexer_service();
        let (enabled, ready, status) = match indexer {
            Some(indexer) => match RpcServerIndexer::indexer_status_json(self, &indexer) {
                Ok(status) => (true, true, status),
                Err(error) => (true, false, json!({ "error": error.to_string() })),
            },
            None => (false, false, Value::Null),
        };
        service_entry_to_json(
            "NeoIndexer",
            interfaces,
            handler_names(RpcServerIndexer::register_handlers()),
            enabled,
            ready,
            status,
        )
    }

    fn oracle_entry(&self) -> Value {
        let oracle = self.oracle_service();
        service_entry_to_json(
            "OracleService",
            &[],
            handler_names(RpcServerOracle::register_handlers()),
            oracle.is_some(),
            oracle.as_ref().is_some_and(|service| service.is_running()),
            oracle.map_or(Value::Null, |service| {
                json!({
                    "status": format!("{:?}", service.status()).to_ascii_lowercase(),
                    "running": service.is_running(),
                    "pendingqueue": service.pending_queue_size(),
                    "inflight": service.in_flight_count(),
                    "dedupcachesize": service.dedup_cache_size(),
                })
            }),
        )
    }
}

fn handler_names(handlers: Vec<RpcHandler>) -> Vec<String> {
    handlers
        .into_iter()
        .map(|handler| handler.descriptor().name.clone())
        .collect()
}
