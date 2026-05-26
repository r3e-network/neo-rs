//! Oracle native contract implementation.
//!
//! The Oracle contract manages external data requests and responses,
//! enabling smart contracts to access off-chain data sources.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::NativeMethod;
use crate::{UInt160, UInt256};

mod config;
mod events;
mod metadata;
mod native_impl;
mod post_persist;
mod pricing;
mod queries;
mod request;
mod response;
mod storage;
mod verification;

pub use config::{OracleConfig, OracleConfigBuilder};

const DEFAULT_PRICE: i64 = 50_000_000;
const PREFIX_PRICE: u8 = 0x05;
const PREFIX_REQUEST: u8 = 0x07;
const PREFIX_ID_LIST: u8 = 0x06;
const PREFIX_REQUEST_ID: u8 = 0x09;
const MAX_PENDING_PER_URL: usize = 256;

#[derive(Debug, Clone)]
struct PendingRequest {
    id: u64,
    original_tx_id: UInt256,
    gas_for_response: i64,
    url: String,
    filter: Option<String>,
    callback_contract: UInt160,
    callback_method: String,
    user_data: Vec<u8>,
}

/// The Oracle native contract.
pub struct OracleContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
    /// Oracle configuration.
    config: OracleConfig,
}

impl OracleContract {
    const ID: i32 = -9;

    /// Creates a new Oracle contract.
    pub fn new() -> Self {
        // Oracle contract hash: 0xfe924b7cfe89ddd271abaf7210a80a7e11178758
        let hash = UInt160::parse("0xfe924b7cfe89ddd271abaf7210a80a7e11178758")
            .expect("Valid OracleContract hash");

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
            config: OracleConfig::default(),
        }
    }

    pub fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        self.dispatch_method(engine, method, args)
    }

    fn invoke_get_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if !args.is_empty() {
            return Err(Error::invalid_operation(
                "getPrice does not accept arguments".to_string(),
            ));
        }
        let snapshot = engine.snapshot_cache();
        Ok(self
            .get_price_value(snapshot.as_ref())
            .to_le_bytes()
            .to_vec())
    }

    fn invoke_finish(&self, engine: &mut ApplicationEngine, _args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.finish(engine)
    }

    fn invoke_verify(&self, engine: &mut ApplicationEngine, _args: &[Vec<u8>]) -> Result<Vec<u8>> {
        self.verify(engine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::DataCache;
    use crate::protocol_settings::ProtocolSettings;
    use crate::smart_contract::native::NativeContract;
    use crate::smart_contract::trigger_type::TriggerType;
    use std::sync::Arc;

    fn application_engine(snapshot: Arc<DataCache>) -> ApplicationEngine {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            ProtocolSettings::default_settings(),
            400_000_000,
            None,
        )
        .expect("application engine")
    }

    #[test]
    fn dispatch_method_covers_declared_metadata_names() {
        let oracle = OracleContract::new();
        let mut engine = application_engine(Arc::new(DataCache::new(false)));
        let mut names = std::collections::BTreeSet::new();

        for method in oracle.methods() {
            if !names.insert(method.name.clone()) {
                continue;
            }

            if let Err(err) = oracle.dispatch_method(&mut engine, &method.name, &[]) {
                assert!(
                    !err.to_string().contains("Unknown method:"),
                    "declared method {} did not dispatch: {err}",
                    method.name
                );
            }
        }

        let err = oracle
            .dispatch_method(&mut engine, "__missing__", &[])
            .expect_err("unknown method");
        assert!(
            err.to_string().contains("Unknown method: __missing__"),
            "unexpected error: {err}"
        );
    }
}
