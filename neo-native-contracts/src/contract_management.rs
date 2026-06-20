//! ContractManagement native contract.
//!
//! Concrete implementation of the ContractManagement native contract:
//! the read-side surface (look up a deployed contract by hash / id),
//! the `deploy` / `update` writers (NEF and manifest validation, the
//! record and id-index writes, the `_deploy` callback and the
//! Deploy/Update events), and the `destroy` writer. The read surface
//! is consumed by oracle service, RPC, the application engine, and
//! the tokens tracker, so it lives here so all those crates can share
//! it without depending on `neo-blockchain`.

use crate::catalog::standard_native_contracts;
use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, ContractState, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::{FindOptions, UInt160};
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use num_bigint::BigInt;
use std::sync::{Arc, LazyLock};

mod metadata;
mod operations;

pub(crate) const CONTRACT_DEPLOY_EVENT: &str = "Deploy";
pub(crate) const CONTRACT_UPDATE_EVENT: &str = "Update";
pub(crate) const CONTRACT_DESTROY_EVENT: &str = "Destroy";

/// Storage prefix for the minimum-deployment-fee setting (C#
/// `ContractManagement.Prefix_MinimumDeploymentFee`).
const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
/// C# default minimum deployment fee: 10 GAS, in datoshi.
const DEFAULT_MINIMUM_DEPLOYMENT_FEE: i64 = 10_00000000;

/// Storage prefix for the per-contract record (matches C#
/// `ContractManagement.PREFIX_CONTRACT`).
const PREFIX_CONTRACT: u8 = 8;
/// Storage prefix for the contract-id → hash index (matches C#
/// `ContractManagement.PREFIX_CONTRACT_HASH`).
const PREFIX_CONTRACT_HASH: u8 = 12;
/// Storage prefix for the next-available-contract-id counter (matches C#
/// `ContractManagement.Prefix_NextAvailableId`).
const PREFIX_NEXT_AVAILABLE_ID: u8 = 15;
/// C# genesis value for `Prefix_NextAvailableId` (`InitializeAsync` writes 1).
const DEFAULT_NEXT_AVAILABLE_ID: i64 = 1;

native_contract_handle!(
    /// Static accessor for the ContractManagement native contract.
    pub struct ContractManagement {
        id: -1,
        contract_name: "ContractManagement",
        hash: CONTRACT_MANAGEMENT_HASH,
    }
);

/// The canonical native-contract registration list (C#
/// `NativeContract.Contracts` order: ContractManagement, StdLib, CryptoLib,
/// Ledger, NEO, GAS, Policy, RoleManagement, Oracle, Notary, Treasury), the
/// iteration order of `ContractManagement::on_persist`. Built directly from
/// the same canonical catalog the provider registers, so the deployment records
/// and `Deploy`/`Update` notifications follow C#'s contract order without
/// making ContractManagement depend on provider lookup plumbing.
static NATIVE_CONTRACTS: LazyLock<Vec<Arc<dyn NativeContract>>> =
    LazyLock::new(standard_native_contracts);

impl NativeContract for ContractManagement {
    native_contract_identity!(ContractManagement);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::CONTRACT_MANAGEMENT_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::CONTRACT_MANAGEMENT_EVENTS
    }

    /// C# `ContractManagement.InitializeAsync(engine, hardfork)` for `hardfork
    /// == ActiveIn` (ContractManagement.cs:53-61; the contract is
    /// genesis-active, so this runs while persisting block 0): seed
    /// `Prefix_MinimumDeploymentFee` (10 GAS) and `Prefix_NextAvailableId` (1).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            Self::minimum_deployment_fee_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        snapshot.add(
            Self::next_available_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_NEXT_AVAILABLE_ID,
            ))),
        );
        Ok(())
    }

    /// C# `ContractManagement.OnPersistAsync` (ContractManagement.cs:71-118):
    /// for every native contract whose `IsInitializeBlock` hits the persisting
    /// block, write (or refresh) its deployment record and emit
    /// `Deploy`/`Update`:
    ///
    /// - no record yet → add the `Prefix_Contract` record (the C#
    ///   interoperable `ContractState` encoding) and the big-endian
    ///   `Prefix_ContractHash` id→hash index entry, then notify `Deploy`;
    /// - record exists (a hardfork refresh) → bump `UpdateCounter` and swap in
    ///   the NEF + manifest composed for this block height (id and hash
    ///   unchanged), then notify `Update`;
    /// - between the record write and the notification, run
    ///   `InitializeAsync(engine, null)` for newly-created genesis-active
    ///   natives and `InitializeAsync(engine, hf)` for every hardfork scheduled
    ///   at this block. Parameterless [`NativeContract::initialize`] models
    ///   the C# `hardfork == ActiveIn` branch; [`initialize_native_for_hardfork`]
    ///   models the non-`ActiveIn` refresh branches such as Policy's
    ///   Echidna/Faun updates.
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let settings = engine.protocol_settings().clone();
        let block_index = engine
            .persisting_block()
            .map(|block| block.index())
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::on_persist requires a persisting block",
                )
            })?;

        for contract in NATIVE_CONTRACTS.iter() {
            let (hit, hardforks) = contract.is_initialize_block(&settings, block_index);
            if !hit {
                continue;
            }
            // C# `contract.GetContractState(settings, index)`: the NEF +
            // manifest composed for this block height.
            let composed = neo_execution::native_contract::build_native_contract_state(
                contract.as_ref(),
                &settings,
                block_index,
            );
            let record_key = Self::contract_storage_key(&contract.hash());
            let snapshot = engine.snapshot_cache();
            let existing = snapshot.get(&record_key);
            let is_create = existing.is_none();
            match existing {
                None => {
                    // Create the contract record + the id → hash index entry.
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
                    if contract.active_in().is_none() {
                        contract.initialize(engine).map_err(|e| {
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
                if Some(*hardfork) == contract.active_in() {
                    contract.initialize(engine).map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::on_persist: initialize {} for {hardfork:?} at block {block_index}: {e}",
                            contract.name()
                        ))
                    })?;
                } else {
                    self.initialize_native_for_hardfork(engine, contract.as_ref(), *hardfork)?;
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

    /// Resolves a deployed contract's state from storage.
    ///
    /// ContractManagement owns the per-contract records, so it backs the
    /// engine's `fetch_contract` storage path (via the native-contract
    /// provider seam): `System.Contract.Call` to any deployed contract —
    /// native or user — resolves its NEF/manifest through here. Delegates to
    /// the read helper used by the `getContract` invoke arm.
    fn lookup_contract_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Self::get_contract_from_snapshot(snapshot, hash)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getContract" => {
                let hash = Self::parse_hash_arg(args, "getContract")?;
                // C# `GetContract` returns the ContractState (as an Array via
                // ToStackItem) or null on miss; the native return marshaling
                // encodes a null Array result as an empty payload.
                match Self::get_contract_from_snapshot(&snapshot, &hash)? {
                    Some(state) => Self::contract_state_to_bytes(&state, "getContract"),
                    None => Ok(Vec::new()),
                }
            }
            "getContractById" => {
                // C# `GetContractById` maps the id to a hash via the
                // contract-id index, then returns that ContractState (or null).
                let id = crate::args::raw_i32_arg(args, 0, "ContractManagement::getContractById")?;
                match Self::get_contract_by_id_from_snapshot(&snapshot, id)? {
                    Some(state) => Self::contract_state_to_bytes(&state, "getContractById"),
                    None => Ok(Vec::new()),
                }
            }
            "getMinimumDeploymentFee" => {
                let fee = self.read_minimum_deployment_fee(&snapshot)?;
                Ok(BigInt::from(fee).to_signed_bytes_le())
            }
            "setMinimumDeploymentFee" => {
                // C#: validate value >= 0 -> AssertCommittee -> overwrite
                // Prefix_MinimumDeploymentFee (stored as the full BigInteger).
                let value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::setMinimumDeploymentFee requires a value",
                        )
                    })?;
                if value < BigInt::from(0) {
                    return Err(CoreError::invalid_operation(
                        "MinimumDeploymentFee cannot be negative",
                    ));
                }
                crate::committee::assert_committee(engine, "setMinimumDeploymentFee")?;
                self.put_minimum_deployment_fee(&engine.snapshot_cache(), &value)?;
                Ok(Vec::new())
            }
            "getContractHashes" => {
                // C# GetContractHashes: an iterator over Prefix_ContractHash with
                // FindOptions.RemovePrefix and prefix length 1, yielding
                // Struct[id_bytes, hash]. The 4-byte iterator id is decoded back
                // into an InteropInterface (StorageIterator) by the dispatcher.
                let results = self.contract_hash_entries(&snapshot);
                let iterator_id = engine
                    .create_storage_iterator_with_options(results, 1, FindOptions::RemovePrefix)
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::getContractHashes: {e}"
                        ))
                    })?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            "isContract" => {
                let hash = Self::parse_hash_arg(args, "isContract")?;
                // C# `IsContract` = snapshot.Contains(key(Prefix_Contract, hash)).
                Ok(vec![u8::from(Self::is_contract(&snapshot, &hash))])
            }
            "hasMethod" => {
                let hash = Self::parse_hash_arg(args, "hasMethod")?;
                let method_name = crate::args::raw_string_arg(
                    args,
                    1,
                    "ContractManagement::hasMethod",
                    "method name",
                )?;
                let pcount = crate::args::raw_i32_arg(args, 2, "ContractManagement::hasMethod")?;
                // C#: false if the contract does not exist; otherwise whether its
                // manifest ABI declares the (method, pcount) method.
                let has = match Self::get_contract_from_snapshot(&snapshot, &hash)? {
                    Some(state) => Self::abi_has_method(&state.manifest, &method_name, pcount),
                    None => false,
                };
                Ok(vec![u8::from(has)])
            }
            // Both deploy overloads land here; args.len() (2 vs 3) selects
            // the C# overload — the 2-arg form forwards data = StackItem.Null.
            "deploy" => self.deploy(engine, args),
            // Both update overloads land here, same overload convention.
            "update" => self.update(engine, args),
            "destroy" => {
                // C# Destroy (~382): the CALLING contract destroys itself
                // (hash = engine.CallingScriptHash; a missing calling context
                // is the C# null-deref fault).
                let hash = engine.get_calling_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "ContractManagement::destroy requires a calling contract",
                    )
                })?;
                // C#: `if (contract is null) return;` — a non-contract caller
                // is a successful no-op.
                let Some(contract) = Self::get_contract_from_snapshot(&snapshot, &hash)? else {
                    return Ok(Vec::new());
                };
                // Delete the per-contract record and the id -> hash index entry.
                snapshot.delete(&Self::contract_storage_key(&hash));
                snapshot.delete(&Self::contract_id_storage_key(contract.id));
                // Delete ALL of the contract's own storage (C# Find over
                // `StorageKey.CreateSearchPrefix(contract.Id, empty)`).
                let search_prefix = StorageKey::new(contract.id, Vec::new());
                let keys: Vec<StorageKey> = snapshot
                    .find(Some(&search_prefix), SeekDirection::Forward)
                    .map(|(key, _)| key)
                    .collect();
                for key in keys {
                    snapshot.delete(&key);
                }
                // C#: `await Policy.BlockAccountInternal(engine, hash)` — lock
                // the destroyed contract (the bool result is discarded) — then
                // `Policy.CleanWhitelist(engine, contract)`.
                crate::PolicyContract::new().block_account_internal(engine, &hash)?;
                self.policy_clean_whitelist(engine, &contract)?;
                // Emit the Destroy event with the destroyed hash.
                engine
                    .send_notification(
                        Self::script_hash(),
                        CONTRACT_DESTROY_EVENT.to_owned(),
                        vec![StackItem::from_byte_string(hash.to_bytes())],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::destroy: notify: {e}"
                        ))
                    })?;
                Ok(Vec::new())
            }
            other => Err(CoreError::invalid_operation(format!(
                "ContractManagement method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
