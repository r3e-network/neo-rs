use neo_indexer::{IndexerError, IndexerService, IndexerStatus};
use neo_native_contracts::LedgerContract;
use serde_json::{Value, json};

use super::RpcServerIndexer;
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
        json!({
            "indexedheight": status.indexed_height,
            "indexedhash": status.indexed_hash.map(|hash| hash.to_string()),
            "indexedblocks": status.indexed_blocks,
            "indexedtransactions": status.indexed_transactions,
            "indexedaccounts": status.indexed_accounts,
            "indexednotifications": status.indexed_notifications,
            "indexednotificationaccounts": status.indexed_notification_accounts,
            "ledgerheight": ledger_height,
            "blocksbehind": status.blocks_behind(ledger_height),
            "synced": status.is_synced_with(ledger_height),
            "applicationlogs": Self::application_logs_status_json(server),
            "persistent": service.is_persistent(),
            "persistencemode": service.persistence_mode(),
            "snapshotpath": service.snapshot_path().map(|path| path.display().to_string()),
            "storepath": service.store_path().map(|path| path.display().to_string()),
        })
    }

    fn ledger_height(server: &RpcServer) -> Option<u32> {
        let store = server.system().store_cache();
        LedgerContract::new().current_index(store.data_cache()).ok()
    }

    fn application_logs_status_json(server: &RpcServer) -> Value {
        match server.system().get_service::<ApplicationLogsService>() {
            Some(logs) => {
                let settings = logs.settings();
                json!({
                    "enabled": true,
                    "notificationrecovery": true,
                    "path": settings.path.as_str(),
                    "debug": settings.debug,
                })
            }
            None => json!({
                "enabled": false,
                "notificationrecovery": false,
            }),
        }
    }
}
