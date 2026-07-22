//! ContractManagement native deployment and refresh hook.
//!
//! Keeps canonical native-contract deployment ordering and hardfork refresh
//! sequencing out of the contract root while preserving C# notification order.

use super::{CONTRACT_DEPLOY_EVENT, CONTRACT_UPDATE_EVENT, ContractManagement};
use crate::StandardNativeContract;
use crate::catalog::standard_native_contracts;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, ContractState, NativeContract};
use neo_storage::StorageItem;
use neo_vm::StackItem;
#[cfg(test)]
use std::sync::LazyLock;

/// The canonical native-contract registration list (C#
/// `NativeContract.Contracts` order: ContractManagement, StdLib, CryptoLib,
/// Ledger, NEO, GAS, Policy, RoleManagement, Oracle, Notary, Treasury), the
/// iteration order of `ContractManagement::on_persist`. Built directly from
/// the same canonical catalog the provider registers, so the deployment records
/// and `Deploy`/`Update` notifications follow C#'s contract order without
/// making ContractManagement depend on provider lookup plumbing.
#[cfg(test)]
pub(in crate::contract_management) static NATIVE_CONTRACTS: LazyLock<Vec<StandardNativeContract>> =
    LazyLock::new(standard_native_contracts);

impl ContractManagement {
    /// C# `ContractManagement.OnPersistAsync` (ContractManagement.cs:71-118):
    /// for every native contract whose `IsInitializeBlock` hits the persisting
    /// block, write (or refresh) its deployment record and emit
    /// `Deploy`/`Update`:
    ///
    /// - no record yet -> add the `Prefix_Contract` record (the C#
    ///   interoperable `ContractState` encoding) and the big-endian
    ///   `Prefix_ContractHash` id->hash index entry, then notify `Deploy`;
    /// - record exists (a hardfork refresh) -> bump `UpdateCounter` and swap in
    ///   the NEF + manifest composed for this block height (id and hash
    ///   unchanged), then notify `Update`;
    /// - between the record write and the notification, run
    ///   `InitializeAsync(engine, null)` for newly-created genesis-active
    ///   natives and `InitializeAsync(engine, hf)` for every hardfork scheduled
    ///   at this block. Parameterless [`NativeContract::initialize`] models
    ///   the C# `hardfork == ActiveIn` branch; `initialize_native_for_hardfork`
    ///   models the non-`ActiveIn` refresh branches such as Policy's
    ///   Echidna/Faun updates.
    pub(super) fn on_persist_native<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
    ) -> CoreResult<()> {
        let block_index = engine
            .persisting_block()
            .map(|block| block.index())
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::on_persist requires a persisting block",
                )
            })?;

        // `NativeContract::is_initialize_block` can only match genesis or a
        // configured hardfork height. Ordinary blocks therefore have no
        // ContractManagement work and must not rebuild every contract's
        // method/event hardfork set merely to prove that fact.
        if block_index != 0
            && !engine
                .protocol_settings()
                .hardforks
                .activation_heights()
                .any(|height| height == block_index)
        {
            return Ok(());
        }
        let settings = engine.protocol_settings().clone();

        let native_contracts = standard_native_contracts();
        for contract in native_contracts.iter() {
            let (hit, hardforks) =
                <StandardNativeContract as NativeContract<P>>::is_initialize_block(
                    contract,
                    &settings,
                    block_index,
                );
            if !hit {
                continue;
            }
            // C# `contract.GetContractState(settings, index)`: the NEF +
            // manifest composed for this block height.
            let composed = neo_execution::native_contract::build_native_contract_state_for::<P, _>(
                contract,
                &settings,
                block_index,
            );
            let record_key = Self::contract_storage_key(&contract.hash());
            let snapshot = engine.snapshot_cache();
            let existing = snapshot.get(&record_key);
            let is_create = existing.is_none();
            match existing {
                None => {
                    // Create the contract record + the id -> hash index entry.
                    snapshot.add(
                        record_key,
                        StorageItem::from_bytes(Self::serialize_contract_record(&composed)?),
                    );
                    snapshot.add(
                        Self::contract_id_storage_key(contract.id()),
                        StorageItem::from_bytes(contract.hash().to_bytes().to_vec()),
                    );

                    // C# create branch: if the native is genesis-active,
                    // `InitializeAsync(engine, null)` runs before the Deploy
                    // notification for this contract.
                    if <StandardNativeContract as NativeContract<P>>::active_in(contract).is_none()
                    {
                        <StandardNativeContract as NativeContract<P>>::initialize(contract, engine).map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "ContractManagement::on_persist: initialize {} at block {block_index}: {e}",
                                contract.name()
                            ))
                        })?;
                    }
                }
                Some(item) => {
                    // C#: UpdateCounter++ and the NEF/manifest swap on the
                    // stored record (id, hash, and the id index unchanged).
                    let mut stored = ContractState::deserialize_contract_record(
                        &item.value_bytes(),
                    )
                    .map_err(|e| {
                        CoreError::deserialization(format!(
                            "ContractManagement::on_persist: stored record for {}: {e}",
                            contract.name()
                        ))
                    })?;
                    // C# `oldContract.UpdateCounter++` is unchecked ushort math.
                    stored.update_counter = stored.update_counter.wrapping_add(1);
                    stored.nef = composed.nef;
                    stored.manifest = composed.manifest;
                    snapshot.update(
                        record_key,
                        StorageItem::from_bytes(Self::serialize_contract_record(&stored)?),
                    );
                }
            }

            // C# `foreach (var hf in hfs) await contract.InitializeAsync(engine, hf)`.
            // The `hf == ActiveIn` branch is represented by `initialize()`;
            // other hardfork refresh branches are dispatched explicitly.
            for hardfork in &hardforks {
                if Some(*hardfork)
                    == <StandardNativeContract as NativeContract<P>>::active_in(contract)
                {
                    <StandardNativeContract as NativeContract<P>>::initialize(contract, engine).map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::on_persist: initialize {} for {hardfork:?} at block {block_index}: {e}",
                            contract.name()
                        ))
                    })?;
                } else {
                    self.initialize_native_for_hardfork(engine, contract, *hardfork)?;
                }
            }

            engine
                .send_notification(
                    Self::script_hash(),
                    if is_create {
                        CONTRACT_DEPLOY_EVENT
                    } else {
                        CONTRACT_UPDATE_EVENT
                    }
                    .to_owned(),
                    vec![StackItem::from_byte_string(contract.hash().to_bytes())],
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "ContractManagement::on_persist: notify for {}: {e}",
                        contract.name()
                    ))
                })?;
        }
        Ok(())
    }
}
