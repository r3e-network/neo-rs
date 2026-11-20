//! Oracle native contract implementation.
//!
//! The Oracle contract manages external data requests and responses,
//! enabling smart contracts to access off-chain data sources.
//!
//! Migrated from `neo-core/src/smart_contract/native/oracle_contract/mod.rs`.
//! Import adaptation: `crate::error::*` → `neo_smart_contract::error::*`;
//! `crate::smart_contract::application_engine::*` →
//! `neo_smart_contract::application_engine::*`; `crate::smart_contract::native::*`
//! → `crate::native::*`; `crate::{UInt160, UInt256}` →
//! `neo_primitives::{UInt160, UInt256}`.

use crate::native::NativeMethod;
use neo_primitives::{UInt160, UInt256};
use neo_smart_contract::application_engine::ApplicationEngine;
use neo_smart_contract::error::{CoreError as Error, CoreResult as Result};

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

/// Maximum request URL length, in bytes (C# `OracleContract.MaxUrlLength`).
pub(super) const MAX_URL_LENGTH: usize = 256;
/// Maximum filter length, in bytes (C# `OracleContract.MaxFilterLength`).
pub(super) const MAX_FILTER_LENGTH: usize = 128;
/// Maximum callback method length, in bytes (C# `OracleContract.MaxCallbackLength`).
pub(super) const MAX_CALLBACK_LENGTH: usize = 32;
/// Maximum serialized user-data length, in bytes (C# `OracleContract.MaxUserDataLength`).
///
/// C# enforces this against the *serialized* `StackItem`
/// (`BinarySerializer.Serialize(userData, MaxUserDataLength, ...)`), so a
/// `ByteString` payload's usable maximum is 512 minus the type byte and the
/// variable-length size prefix. Enforcing it against the raw payload keeps us
/// within one varint-prefix of the reference behaviour instead of the previous
/// 128x-too-permissive `BLOCK_MAX_TX_WIRE_LIMIT` (65_535), which let this node
/// accept Oracle requests every C# node rejects.
pub(super) const MAX_USER_DATA_LENGTH: usize = 512;

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
    methods: &'static [NativeMethod],
    /// Oracle configuration.
    config: OracleConfig,
}

impl OracleContract {
    const ID: i32 = -9;

    /// Creates a new Oracle contract.
    pub fn new() -> Self {
        // Oracle contract hash: 0xfe924b7cfe89ddd271abaf7210a80a7e11178758
        let hash =
            crate::native::static_contract_hash!("0xfe924b7cfe89ddd271abaf7210a80a7e11178758");

        Self {
            id: Self::ID,
            hash,
            methods: crate::native::static_native_methods!(OracleContract::native_methods()),
            config: OracleConfig::default(),
        }
    }

    /// Dispatches an Oracle native contract method invocation.
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
        // C# `getPrice` returns the stored BigInteger; keep the variable-width
        // signed-LE encoding instead of narrowing to a fixed 8-byte i64.
        Ok(self.get_price_value(snapshot.as_ref()).to_signed_bytes_le())
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
    use crate::native::NativeContract;
    use neo_primitives::CallFlags;
    use neo_primitives::ContractParameterType;
    use neo_primitives::TriggerType;
    use neo_protocol::ProtocolSettings;
    use neo_storage::persistence::DataCache;
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
            crate::native::composition::default_native_services(),
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

    /// Builds a PostPersist engine that observes `block` for the given height.
    fn post_persist_engine(
        snapshot: Arc<DataCache>,
        header_index: u32,
        transactions: Vec<neo_protocol::payloads::Transaction>,
    ) -> ApplicationEngine {
        let mut header = neo_protocol::payloads::Header::new();
        header.set_index(header_index);
        header.set_prev_hash(UInt256::zero());
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(u64::from(header_index) + 1);
        header.witness = neo_protocol::witness::Witness::new();
        let block = neo_protocol::payloads::Block {
            header,
            transactions,
        };
        ApplicationEngine::new(
            TriggerType::PostPersist,
            None,
            snapshot,
            Some(block),
            ProtocolSettings::testnet(),
            i64::MAX,
            None,
            crate::native::composition::default_native_services(),
        )
        .expect("post-persist engine")
    }

    /// Tracked storage effects, sorted by key for stable comparison.
    fn storage_fingerprint(
        snapshot: &DataCache,
    ) -> Vec<(Vec<u8>, neo_storage::TrackState, Vec<u8>)> {
        let mut tracked: Vec<_> = snapshot
            .tracked_items()
            .into_iter()
            .map(|(key, trackable)| (key.to_array(), trackable.state, trackable.item.to_value()))
            .collect();
        tracked.sort_by(|left, right| left.0.cmp(&right.0));
        tracked
    }

    /// Seeds one pending request (plus its URL id-list entry) and returns the
    /// storage hash H160 of the request URL.
    fn seed_pending_request(oracle: &OracleContract, snapshot: &DataCache, id: u64) -> [u8; 20] {
        let request = PendingRequest {
            id,
            original_tx_id: UInt256::zero(),
            gas_for_response: 0,
            url: "http://localhost:10332/".to_string(),
            filter: None,
            callback_contract: UInt160::zero(),
            callback_method: "callback".to_string(),
            user_data: Vec::new(),
        };
        oracle
            .write_request(snapshot, &request)
            .expect("write pending request");
        let url_hash = oracle.compute_url_hash(&request.url);
        oracle
            .append_request_id(snapshot, &url_hash, id)
            .expect("append request id");
        url_hash
    }

    fn oracle_response_transaction(id: u64) -> neo_protocol::payloads::Transaction {
        let mut transaction = neo_protocol::payloads::Transaction::new();
        transaction.set_nonce(3);
        transaction.set_system_fee(1_000_000);
        transaction.set_valid_until_block(100);
        transaction.add_attribute(
            neo_protocol::payloads::TransactionAttribute::OracleResponse(
                neo_protocol::payloads::OracleResponse::new(
                    id,
                    neo_protocol::payloads::OracleResponseCode::Success,
                    vec![],
                ),
            ),
        );
        transaction.set_script(vec![]);
        transaction
    }

    // Differential guard for the Phase 4B cleanup fast path: an empty block has
    // no OracleResponse attribute, so the pre-optimization body read nothing and
    // deleted nothing. The early return must reproduce that exactly, and must not
    // reward oracle nodes either.
    #[test]
    fn post_persist_on_an_empty_block_leaves_pending_requests_untouched() {
        let oracle = OracleContract::new();
        let snapshot = Arc::new(DataCache::new(false));
        let url_hash = seed_pending_request(&oracle, snapshot.as_ref(), 7);
        let before = storage_fingerprint(&snapshot);

        let mut engine = post_persist_engine(Arc::clone(&snapshot), 1, Vec::new());
        oracle.post_persist(&mut engine).expect("post persist");

        assert_eq!(
            before,
            storage_fingerprint(&snapshot),
            "an empty block must not mutate oracle storage"
        );
        assert!(
            engine.notifications().is_empty(),
            "an empty block must not mint oracle rewards"
        );
        assert!(
            oracle
                .read_request(snapshot.as_ref(), 7)
                .expect("read request")
                .is_some(),
            "pending request must survive an empty block"
        );
        assert_eq!(
            oracle
                .read_id_list(snapshot.as_ref(), &url_hash)
                .expect("id list"),
            vec![7],
            "pending id list must survive an empty block"
        );
    }

    // The same fast path must still clean up an answered request; ordering and
    // the id-list maintenance are the consensus-visible part of this hook.
    #[test]
    fn post_persist_still_cleans_up_an_answered_request() {
        let oracle = OracleContract::new();
        let snapshot = Arc::new(DataCache::new(false));
        let url_hash = seed_pending_request(&oracle, snapshot.as_ref(), 7);
        let before = storage_fingerprint(&snapshot);

        let mut engine = post_persist_engine(
            Arc::clone(&snapshot),
            1,
            vec![oracle_response_transaction(7)],
        );
        oracle.post_persist(&mut engine).expect("post persist");

        assert!(
            oracle
                .read_request(snapshot.as_ref(), 7)
                .expect("read request")
                .is_none(),
            "answered request must be deleted"
        );
        assert!(
            oracle
                .read_id_list(snapshot.as_ref(), &url_hash)
                .expect("id list")
                .is_empty(),
            "answered request must leave the pending id list"
        );
        assert!(
            before != storage_fingerprint(&snapshot),
            "cleaning up an answered request must mutate storage"
        );
    }

    // A response id without a stored request is the `read_request -> None` path;
    // it must stay a no-op rather than an error.
    #[test]
    fn post_persist_ignores_unknown_response_ids() {
        let oracle = OracleContract::new();
        let snapshot = Arc::new(DataCache::new(false));
        let before = storage_fingerprint(&snapshot);

        let mut engine = post_persist_engine(
            Arc::clone(&snapshot),
            1,
            vec![oracle_response_transaction(9)],
        );
        oracle.post_persist(&mut engine).expect("post persist");

        assert_eq!(
            before,
            storage_fingerprint(&snapshot),
            "an unknown response id must not mutate oracle storage"
        );
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
