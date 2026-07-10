//! Native contract read capabilities used by the oracle service.
//!
//! The oracle service needs narrow read access to Oracle, RoleManagement,
//! Policy, and ContractManagement native-contract storage. Keeping those reads
//! behind a crate-local provider seam prevents service loops and transaction
//! builders from constructing concrete native handles at each call site while
//! preserving the native contracts' canonical storage codecs.

use super::{OracleRuntimeProvider, OracleService};
use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::ContractState;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{OracleContract, OracleRequest};
use neo_primitives::UInt160;
use neo_storage::{CacheRead, DataCache};
use std::sync::Arc;

/// Oracle request storage reads supplied by a concrete native provider.
///
/// This remains local to `neo-oracle-service` because `OracleRequest` is owned
/// by `neo-native-contracts`; moving it into `neo-execution` would reverse the
/// intended crate dependency direction.
pub trait OracleContractReadProvider: NativeContractProvider {
    /// Returns a single pending oracle request by id.
    fn oracle_request<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>>;

    /// Returns all pending oracle requests.
    fn oracle_requests<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<(u64, OracleRequest)>>;

    /// Returns all pending oracle requests for `url`.
    fn oracle_requests_by_url<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>>;
}

impl OracleContractReadProvider for neo_native_contracts::StandardNativeProvider {
    fn oracle_request<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        OracleContract::new().get_request(snapshot, request_id)
    }

    fn oracle_requests<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        Ok(OracleContract::new().get_requests(snapshot))
    }

    fn oracle_requests_by_url<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        OracleContract::new().get_requests_by_url(snapshot, url)
    }
}

/// Native-contract read capabilities required by oracle processing.
pub(super) trait OracleServiceNativeProvider {
    /// Returns the Oracle native contract hash.
    fn oracle_hash(&self) -> CoreResult<UInt160>;

    /// Returns the deployed Oracle native contract state, when present.
    fn oracle_contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<ContractState>>;

    /// Returns a single pending oracle request by id.
    fn oracle_request<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>>;

    /// Returns all pending oracle requests.
    fn oracle_requests<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<(u64, OracleRequest)>>;

    /// Returns all pending oracle requests for `url`.
    fn oracle_requests_by_url<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>>;

    /// Returns the designated oracle nodes effective at `height`.
    fn designated_oracles<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>>;

    /// Returns the effective max valid-until-block increment.
    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32>;

    /// Returns the effective execution fee factor.
    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32>;

    /// Returns the effective fee per byte.
    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32>;
}

/// Adapter from the node-composed native-contract provider to the oracle
/// service's narrow native read capability.
#[derive(Clone)]
pub(super) struct NativeOracleServiceProvider<P>
where
    P: NativeContractProvider + OracleContractReadProvider,
{
    native_contract_provider: Arc<P>,
}

impl<P> NativeOracleServiceProvider<P>
where
    P: NativeContractProvider + OracleContractReadProvider,
{
    /// Creates an adapter over the composition-root native-contract provider.
    #[must_use]
    pub(super) fn new(native_contract_provider: Arc<P>) -> Self {
        Self {
            native_contract_provider,
        }
    }
}

impl<P> std::fmt::Debug for NativeOracleServiceProvider<P>
where
    P: NativeContractProvider + OracleContractReadProvider,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeOracleServiceProvider")
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl<R, P> OracleService<R, P>
where
    R: OracleRuntimeProvider + 'static,
    P: NativeContractProvider + OracleContractReadProvider + 'static,
{
    pub(super) fn native_provider(&self) -> NativeOracleServiceProvider<P> {
        NativeOracleServiceProvider::new(Arc::clone(&self.native_contract_provider))
    }
}

impl<P> OracleServiceNativeProvider for NativeOracleServiceProvider<P>
where
    P: NativeContractProvider + OracleContractReadProvider,
{
    fn oracle_hash(&self) -> CoreResult<UInt160> {
        self.native_contract_provider.oracle_hash()
    }

    fn oracle_contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<ContractState>> {
        self.native_contract_provider
            .oracle_contract_state(snapshot)
    }

    fn oracle_request<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<OracleRequest>> {
        self.native_contract_provider
            .oracle_request(snapshot, request_id)
    }

    fn oracle_requests<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        self.native_contract_provider.oracle_requests(snapshot)
    }

    fn oracle_requests_by_url<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        url: &str,
    ) -> CoreResult<Vec<(u64, OracleRequest)>> {
        self.native_contract_provider
            .oracle_requests_by_url(snapshot, url)
    }

    fn designated_oracles<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        self.native_contract_provider
            .designated_oracles(snapshot, height)
    }

    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        self.native_contract_provider
            .max_valid_until_block_increment(snapshot, settings)
    }

    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32> {
        self.native_contract_provider
            .exec_fee_factor(snapshot, settings, height)
    }

    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.native_contract_provider.fee_per_byte(snapshot)
    }
}
