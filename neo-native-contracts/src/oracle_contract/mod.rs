//! # neo-native-contracts::oracle_contract
//!
//! Native Oracle contract request, response, and fee behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `invoke`: native method dispatch for request/finish/verify.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `request`: oracle request records and lifecycle helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::ORACLE_CONTRACT_HASH;
use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract::OracleRequestDetails;
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;

mod invoke;
mod metadata;
mod request;
mod storage;

pub use request::OracleRequest;

/// C# `OracleContract.MaxUrlLength` (strict-UTF8 bytes).
const MAX_URL_LENGTH: usize = 256;
/// C# `OracleContract.MaxFilterLength` (strict-UTF8 bytes).
const MAX_FILTER_LENGTH: usize = 128;
/// C# `OracleContract.MaxCallbackLength` (strict-UTF8 bytes).
const MAX_CALLBACK_LENGTH: usize = 32;
/// C# `OracleContract.MaxUserDataLength` (serialized bytes).
const MAX_USER_DATA_LENGTH: usize = 512;

/// Storage prefix for the oracle request price (C# `OracleContract.Prefix_Price`).
const PREFIX_PRICE: u8 = 5;
/// Storage prefix for the per-url request-id list (C# `Prefix_IdList`).
const PREFIX_ID_LIST: u8 = 6;
/// Storage prefix for the pending request records (C# `Prefix_Request`).
const PREFIX_REQUEST: u8 = 7;
/// Storage prefix for the next-request-id counter (C# `Prefix_RequestId`).
const PREFIX_REQUEST_ID: u8 = 9;

/// C# default oracle price: 0.5 GAS, in datoshi (genesis `InitializeAsync` value).
const DEFAULT_ORACLE_PRICE: i64 = 50000000;
/// C# `Request`: `gasForResponse` must be at least 0.1 GAS (`0_10000000` datoshi).
const MIN_GAS_FOR_RESPONSE: i64 = 10000000;
/// C# `Request`: at most 256 pending responses per url.
const MAX_PENDING_IDS_PER_URL: usize = 256;
pub(crate) const ORACLE_REQUEST_EVENT: &str = "OracleRequest";
pub(crate) const ORACLE_RESPONSE_EVENT: &str = "OracleResponse";

native_contract_handle!(
    /// Static accessor for the OracleContract native contract.
    pub struct OracleContract {
        id: -9,
        contract_name: "OracleContract",
        hash: ORACLE_CONTRACT_HASH,
    }
);

impl NativeContract for OracleContract {
    native_contract_identity!(OracleContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::ORACLE_CONTRACT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::ORACLE_CONTRACT_EVENTS
    }

    /// C# `OracleContract.Activations => [null, HF_Faun]` (OracleContract.cs:56):
    /// active from genesis, but the manifest's supported standards update at
    /// Faun, so the Faun boundary must refresh the stored contract state.
    fn activations(&self) -> &'static [Hardfork] {
        &[Hardfork::HfFaun]
    }

    /// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): NEP-30
    /// once HF_Faun is enabled at the height; no standards before it.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            crate::native_supported_standards(&[crate::NEP30_STANDARD])
        } else {
            Vec::new()
        }
    }

    /// Url + original txid pair consumed by the engine's oracle-response
    /// witness path (`CheckWitness` signer inheritance).
    ///
    /// C# `GetRequest(...)` exposed through the native-contract seam so the
    /// engine can resolve oracle-response witnesses without depending on
    /// `neo-native-contracts`.
    fn oracle_request_url_full(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(self
            .read_request(snapshot, id)?
            .map(|request| OracleRequestDetails::new(request.url, request.original_tx_id)))
    }

    /// C# `OracleContract.InitializeAsync(engine, hardfork)` for
    /// `hardfork == ActiveIn` (the Oracle contract is genesis-active): seed the
    /// request-id counter with `BigInteger.Zero` (stored as empty bytes) and the
    /// request price with 0.5 GAS (`0_50000000` datoshi).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::request_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(0))),
        );
        snapshot.add(
            Self::price_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_ORACLE_PRICE,
            ))),
        );
        Ok(())
    }

    /// C# `OracleContract.PostPersistAsync`: for every oracle-response
    /// transaction in the persisting block, remove the answered request
    /// record and its id from the per-url id-list, then mint the oracle
    /// price to the designated oracle node selected by `id % nodes.len()`.
    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let (block_index, response_ids): (u32, Vec<u64>) = {
            let block = crate::support::engine::require_persisting_block(
                engine,
                "OracleContract::post_persist",
            )?;
            let ids = block
                .transactions
                .iter()
                .filter_map(|tx| Self::oracle_response_attribute(tx).map(|response| response.id))
                .collect();
            (block.index(), ids)
        };

        let snapshot = engine.snapshot_cache();
        let mut nodes: Option<Vec<(UInt160, BigInt)>> = None;
        for id in response_ids {
            // Remove the request from storage (skip responses without one).
            let key = Self::request_key(id);
            let Some(item) = snapshot.get(&key) else {
                continue;
            };
            let request = Self::decode_oracle_request(&item.value_bytes())?;
            snapshot.delete(&key);

            // Remove the id from the url id-list; C# throws when the id is
            // not listed, and deletes the entry once the list is empty.
            let list_key = Self::id_list_key(&request.url);
            let mut list = match snapshot.get(&list_key) {
                Some(list_item) => Self::decode_id_list(&list_item.value_bytes())?,
                None => Vec::new(),
            };
            let Some(position) = list.iter().position(|listed| *listed == id) else {
                return Err(CoreError::invalid_operation(
                    "OracleContract::post_persist: request id missing from the url id-list",
                ));
            };
            list.remove(position);
            if list.is_empty() {
                snapshot.delete(&list_key);
            } else {
                snapshot.update(
                    list_key,
                    StorageItem::from_bytes(Self::encode_id_list(&list)?),
                );
            }

            // Accumulate the oracle fee for the node selected by the id.
            if nodes.is_none() {
                let points = crate::RoleManagement::new().get_designated_by_role_at(
                    &snapshot,
                    crate::Role::Oracle,
                    block_index,
                )?;
                nodes = Some(
                    points
                        .into_iter()
                        .map(|point| {
                            (
                                UInt160::from_script(&Contract::create_signature_redeem_script(
                                    point,
                                )),
                                BigInt::from(0),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(nodes) = nodes.as_mut() {
                if !nodes.is_empty() {
                    let index = usize::try_from(id % nodes.len() as u64).unwrap_or(0);
                    let price = self.read_price(&snapshot)?;
                    nodes[index].1 += BigInt::from(price);
                }
            }
        }

        if let Some(nodes) = nodes {
            for (account, gas) in nodes {
                if gas > BigInt::from(0) {
                    crate::GasToken::new().gas_mint(engine, &account, &gas, false)?;
                }
            }
        }
        Ok(())
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }
}

#[cfg(test)]
#[path = "../tests/oracle_contract/mod.rs"]
mod tests;
