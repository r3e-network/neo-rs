use neo_indexer::{IndexerError, IndexerService, IndexerStatus};
use neo_native_contracts::LedgerContract;
use serde_json::Value;

use super::RpcServerIndexer;
use super::responses::ApplicationLogsStatus;
use crate::application_logs::ApplicationLogsService;
use crate::server::rpc_server::RpcServer;

impl RpcServerIndexer {
    pub(crate) fn indexer_status_json(
        server: &RpcServer,
        service: &IndexerService,
    ) -> Result<Value, IndexerError> {
        let status = service.try_status()?;
        Ok(Self::status_json(server, service, status))
    }

    fn status_json(server: &RpcServer, service: &IndexerService, status: IndexerStatus) -> Value {
        let ledger_height = Self::ledger_height(server);
        let application_logs = Self::application_logs_status(server);
        Self::indexer_status_to_json(service, status, ledger_height, application_logs)
    }

    fn ledger_height(server: &RpcServer) -> Option<u32> {
        let store = server.system().store_cache();
        LedgerContract::new().current_index(store.data_cache()).ok()
    }

    fn application_logs_status(server: &RpcServer) -> ApplicationLogsStatus {
        match server.system().get_service::<ApplicationLogsService>() {
            Some(logs) => {
                let settings = logs.settings();
                ApplicationLogsStatus {
                    enabled: true,
                    notification_recovery: true,
                    path: Some(settings.path.clone()),
                    debug: Some(settings.debug),
                }
            }
            None => ApplicationLogsStatus {
                enabled: false,
                notification_recovery: false,
                path: None,
                debug: None,
            },
        }
    }
}
