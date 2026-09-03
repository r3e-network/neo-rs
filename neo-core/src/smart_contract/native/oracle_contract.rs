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
    use crate::smart_contract::call_flags::CallFlags;
    use crate::smart_contract::native::NativeContract;
    use crate::smart_contract::trigger_type::TriggerType;
    use crate::smart_contract::ContractParameterType;
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
    fn native_methods_match_oracle_protocol_metadata() {
        let oracle = OracleContract::new();
        let methods = oracle.methods();

        assert_eq!(methods.len(), 5);

        let request = &methods[0];
        assert_eq!(request.name, "request");
        assert_eq!(request.cpu_fee, 0);
        assert_eq!(request.storage_fee, 0);
        assert!(!request.safe);
        assert_eq!(
            request.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            request.parameters,
            vec![
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::String,
                ContractParameterType::Any,
                ContractParameterType::Integer,
            ]
        );
        assert_eq!(
            request.parameter_names,
            vec!["url", "filter", "callback", "userData", "gasForResponse"]
        );
        assert_eq!(request.return_type, ContractParameterType::Void);

        let get_price = &methods[1];
        assert_eq!(get_price.name, "getPrice");
        assert_eq!(get_price.cpu_fee, 1 << 15);
        assert!(get_price.safe);
        assert_eq!(get_price.required_call_flags, CallFlags::READ_STATES.bits());
        assert!(get_price.parameters.is_empty());
        assert_eq!(get_price.return_type, ContractParameterType::Integer);

        let set_price = &methods[2];
        assert_eq!(set_price.name, "setPrice");
        assert_eq!(set_price.cpu_fee, 1 << 15);
        assert!(!set_price.safe);
        assert_eq!(set_price.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(set_price.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(set_price.parameter_names, vec!["price"]);
        assert_eq!(set_price.return_type, ContractParameterType::Void);

        let finish = &methods[3];
        assert_eq!(finish.name, "finish");
        assert_eq!(finish.cpu_fee, 0);
        assert!(!finish.safe);
        assert_eq!(
            finish.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert!(finish.parameters.is_empty());
        assert_eq!(finish.return_type, ContractParameterType::Void);

        let verify = &methods[4];
        assert_eq!(verify.name, "verify");
        assert_eq!(verify.cpu_fee, 1 << 15);
        assert!(verify.safe);
        assert_eq!(verify.required_call_flags, 0);
        assert!(verify.parameters.is_empty());
        assert_eq!(verify.return_type, ContractParameterType::Boolean);

        assert!(methods.iter().all(|method| method.active_in.is_none()));
        assert!(methods.iter().all(|method| method.deprecated_in.is_none()));
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
