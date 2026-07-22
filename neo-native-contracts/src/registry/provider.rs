//! Standard native-contract provider.
//!
//! Implements neo-execution's [`NativeContractProvider`] seam over the concrete
//! native contracts in this crate. Composition roots pass this provider into
//! engines and services so `ApplicationEngine` can dispatch `System.Contract.Call`
//! to a native contract without `neo-execution` depending on
//! `neo-native-contracts` (which would be a crate cycle).
//!
//! The canonical catalog in [`crate::catalog`] is the single source of truth for
//! standard-contract order, id, name, hash, and construction.

use neo_config::ProtocolSettings;
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_execution::{ContractState, NativeContract};
use neo_payloads::{TransactionState, TrimmedBlock};
use neo_primitives::{TransactionAttributeType, UInt160, UInt256};
use neo_storage::{CacheRead, DataCache};
use num_bigint::BigInt;

use crate::catalog::{
    StandardNativeContractSpec, standard_native_contract_hashes,
    standard_native_contract_spec_by_hash, standard_native_contracts,
};
use crate::{
    ContractManagement, GasToken, LedgerContract, NeoToken, Notary, OracleContract, PolicyContract,
    Role, RoleManagement, StandardNativeContract,
};

/// Provider over every standard native contract, in canonical C# id order.
pub struct StandardNativeProvider {
    contracts: Vec<StandardNativeContract>,
}

impl StandardNativeProvider {
    /// Builds the provider from the canonical standard native-contract catalog.
    pub fn new() -> Self {
        Self {
            contracts: standard_native_contracts(),
        }
    }

    fn contract_for_spec(
        &self,
        spec: StandardNativeContractSpec,
    ) -> Option<StandardNativeContract> {
        self.contracts
            .iter()
            .find(|contract| contract.id() == spec.id)
            .copied()
    }
}

neo_io::impl_default_via_new!(StandardNativeProvider);

impl NativeContractProvider for StandardNativeProvider {
    type Contract = StandardNativeContract;

    fn get_native_contract(&self, hash: &UInt160) -> Option<StandardNativeContract> {
        standard_native_contract_spec_by_hash(hash).and_then(|spec| self.contract_for_spec(spec))
    }

    fn all_native_contracts(&self) -> Vec<StandardNativeContract> {
        self.contracts.clone()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        standard_native_contract_hashes().into_iter().collect()
    }

    fn current_block_index<B: CacheRead>(
        &self,
        snapshot: &neo_storage::DataCache<B>,
    ) -> neo_error::CoreResult<u32> {
        LedgerContract::new().current_index(snapshot)
    }

    fn state_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        index: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        RoleManagement::new().get_designated_by_role_at(snapshot, Role::StateValidator, index)
    }

    fn max_traceable_blocks<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        PolicyContract::new().get_max_traceable_blocks_snapshot(snapshot, settings)
    }

    fn policy_is_blocked<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<bool> {
        Ok(PolicyContract::is_blocked_snapshot(snapshot, account))
    }

    fn policy_whitelisted_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        contract_hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        <PolicyContract as NativeContract<StandardNativeProvider>>::whitelisted_fee(
            &PolicyContract::new(),
            snapshot,
            contract_hash,
            method,
            param_count,
        )
    }

    fn max_valid_until_block_increment<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        PolicyContract::new().get_max_valid_until_block_increment_snapshot(snapshot, settings)
    }

    fn fee_per_byte<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        PolicyContract::new().get_fee_per_byte_snapshot(snapshot)
    }

    fn attribute_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        attribute_type: TransactionAttributeType,
    ) -> CoreResult<i64> {
        PolicyContract::new().attribute_fee(snapshot, attribute_type.to_byte(), true)
    }

    fn exec_fee_factor<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        height: u32,
    ) -> CoreResult<u32> {
        PolicyContract::new().get_exec_fee_factor_snapshot(snapshot, settings, height)
    }

    fn exec_fee_factor_raw<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        PolicyContract::new().get_exec_fee_factor_raw_snapshot(snapshot)
    }

    fn storage_price<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        PolicyContract::new().get_storage_price_snapshot(snapshot)
    }

    fn gas_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        Ok(GasToken::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0)))
    }

    fn notary_balance<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        Ok(Notary::balance_of(snapshot, account).unwrap_or_else(|_| BigInt::from(0)))
    }

    fn notary_hash(&self) -> CoreResult<UInt160> {
        Ok(Notary::new().hash())
    }

    fn committee_address<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<UInt160>> {
        <NeoToken as NativeContract<StandardNativeProvider>>::committee_address(
            &NeoToken::new(),
            snapshot,
        )
    }

    fn next_block_validators<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<Vec<ECPoint>> {
        NeoToken::new().next_block_validators(
            snapshot,
            usize::try_from(settings.validators_count).unwrap_or(0),
        )
    }

    fn designated_oracles<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        height: u32,
    ) -> CoreResult<Vec<ECPoint>> {
        RoleManagement::new().get_designated_by_role_at(snapshot, Role::Oracle, height)
    }

    fn oracle_hash(&self) -> CoreResult<UInt160> {
        Ok(OracleContract::new().hash())
    }

    fn oracle_response_gas<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<i64>> {
        OracleContract::new()
            .get_request(snapshot, request_id)
            .map(|request| request.map(|request| request.gas_for_response))
    }

    fn oracle_contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<ContractState>> {
        ContractManagement::get_contract_from_snapshot(snapshot, &OracleContract::new().hash())
    }

    fn contract_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        ContractManagement::get_contract_from_snapshot(snapshot, hash)
    }

    fn contract_exists<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt160,
    ) -> CoreResult<bool> {
        Ok(ContractManagement::is_contract(snapshot, hash))
    }

    fn oracle_request_details<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        request_id: u64,
    ) -> CoreResult<Option<neo_execution::native_contract::OracleRequestDetails>> {
        <OracleContract as NativeContract<StandardNativeProvider>>::oracle_request_url_full(
            &OracleContract::new(),
            snapshot,
            request_id,
        )
    }

    fn transaction_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<TransactionState>> {
        LedgerContract::new().get_transaction_state(snapshot, tx_hash)
    }

    fn trimmed_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        block_hash: &UInt256,
    ) -> CoreResult<Option<TrimmedBlock>> {
        LedgerContract::new().get_trimmed_block(snapshot, block_hash)
    }

    fn next_consensus_address_for_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        block_index: u32,
    ) -> CoreResult<UInt160> {
        NeoToken::new().next_consensus_address_for_block(snapshot, settings, block_index)
    }

    fn milliseconds_per_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
    ) -> CoreResult<u32> {
        PolicyContract::new().get_milliseconds_per_block_snapshot(snapshot, settings)
    }

    fn fast_forward_empty_block_rewards<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        settings: &ProtocolSettings,
        start: u32,
        end: u32,
    ) -> CoreResult<()> {
        NeoToken::new().fast_forward_empty_block_rewards(snapshot, settings, start, end)
    }
}

#[cfg(test)]
#[path = "../tests/registry/provider.rs"]
mod tests;
