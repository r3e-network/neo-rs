use std::sync::{Arc, Mutex};
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use std::thread;

use crate::config;
use crate::core::block;
use crate::services::blockfetcher::New;
use crate::services::blockfetcher::defaultTimeout;
use crate::services::blockfetcher::defaultOIDBatchSize;
use crate::services::blockfetcher::defaultDownloaderWorkersCount;
use crate::services::blockfetcher::Service;
use zap::Logger;
use zap::NopLogger;

struct MockLedger {
    height: u32,
}

impl MockLedger {
    fn get_config(&self) -> config::Blockchain {
        config::Blockchain {}
    }

    fn block_height(&self) -> u32 {
        self.height
    }
}

struct MockPutBlockFunc {
    put_called: AtomicBool,
}

impl MockPutBlockFunc {
    fn new() -> Self {
        Self {
            put_called: AtomicBool::new(false),
        }
    }

    fn put_block(&self, _b: &block::Block) -> Result<(), Box<dyn Error>> {
        self.put_called.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn test_service_constructor() {
    let logger = NopLogger::new();
    let ledger = Arc::new(MockLedger { height: 10 });
    let mock_put = Arc::new(MockPutBlockFunc::new());
    let shutdown_callback = Arc::new(Mutex::new(|| {}));

    let empty_config = config::NeoFSBlockFetcher {
        timeout: Duration::from_secs(0),
        oid_batch_size: 0,
        downloader_workers_count: 0,
        addresses: vec![],
        internal_service: config::InternalService::default(),
    };

    let no_addresses_config = config::NeoFSBlockFetcher {
        addresses: vec![],
        ..Default::default()
    };

    let default_values_config = config::NeoFSBlockFetcher {
        addresses: vec!["http://localhost:8080".to_string()],
        ..Default::default()
    };

    let invalid_wallet_config = config::NeoFSBlockFetcher {
        addresses: vec!["http://localhost:8080".to_string()],
        internal_service: config::InternalService {
            enabled: true,
            unlock_wallet: config::Wallet {
                path: "invalid/path/to/wallet.json".to_string(),
                password: "wrong-password".to_string(),
            },
        },
        ..Default::default()
    };

    // Test empty configuration
    {
        let result = New(ledger.clone(), empty_config.clone(), logger.clone(), mock_put.clone(), shutdown_callback.clone());
        assert!(result.is_err());
    }

    // Test no addresses
    {
        let result = New(ledger.clone(), no_addresses_config.clone(), logger.clone(), mock_put.clone(), shutdown_callback.clone());
        assert!(result.is_err());
    }

    // Test default values
    {
        let service = New(ledger.clone(), default_values_config.clone(), logger.clone(), mock_put.clone(), shutdown_callback.clone()).unwrap();
        assert!(!service.is_active());
        assert_eq!(service.cfg.timeout, defaultTimeout);
        assert_eq!(service.cfg.oid_batch_size, defaultOIDBatchSize);
        assert_eq!(service.cfg.downloader_workers_count, defaultDownloaderWorkersCount);
        assert!(!service.is_active());
    }

    // Test SDK client
    {
        let service = New(ledger.clone(), default_values_config.clone(), logger.clone(), mock_put.clone(), shutdown_callback.clone()).unwrap();
        let result = service.start();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("create SDK client"));
        assert!(!service.is_active());
    }

    // Test invalid wallet
    {
        let result = New(ledger.clone(), invalid_wallet_config.clone(), logger.clone(), mock_put.clone(), shutdown_callback.clone());
        assert!(result.is_err());
    }
}
