use super::{CONTRACT_DEPLOY_EVENT, CONTRACT_UPDATE_EVENT, ContractManagement};
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::helper::Helper;
use neo_execution::{ApplicationEngine, ContractState, NativeContract};
use neo_payloads::transaction::Transaction;
use neo_primitives::ContractBasicMethod;
use neo_storage::StorageItem;
use neo_vm::StackItem;

mod storage;
mod validation;

impl ContractManagement {
    /// C# `ContractManagement.OnDeployAsync`: invoke the contract's `_deploy(data,
    /// update)` callback when (and only when) its manifest ABI declares it with
    /// exactly two parameters, then emit the `Deploy` / `Update` event.
    ///
    /// The callback goes through `queue_contract_call_from_native` (the faithful
    /// equivalent of C# `CallFromNativeContractAsync` in this engine, proven by
    /// the NEP-17 `onNEP17Payment` path): it executes after the native method
    /// returns, against the record this method has already written, and a fault
    /// inside `_deploy` still faults the whole transaction as in C#.
    pub(super) fn on_deploy(
        &self,
        engine: &mut ApplicationEngine,
        contract: &ContractState,
        data: StackItem,
        update: bool,
    ) -> CoreResult<()> {
        if Self::abi_has_method(
            &contract.manifest,
            ContractBasicMethod::DEPLOY,
            ContractBasicMethod::DEPLOY_P_COUNT,
        ) {
            engine.queue_contract_call_from_native(
                ContractManagement::script_hash(),
                contract.hash,
                ContractBasicMethod::DEPLOY,
                vec![data, StackItem::from_bool(update)],
            );
        }
        let event = if update {
            CONTRACT_UPDATE_EVENT
        } else {
            CONTRACT_DEPLOY_EVENT
        };
        engine
            .send_notification(
                ContractManagement::script_hash(),
                event.to_owned(),
                vec![StackItem::from_byte_string(contract.hash.to_bytes())],
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("ContractManagement: {event} notify: {e}"))
            })
    }

    /// C# `ContractManagement.Deploy(engine, nefFile, manifest, data)` (~239-303):
    /// validates the caller / payloads, charges
    /// `max(StoragePrice * payload, GetMinimumDeploymentFee)`, computes the
    /// contract hash from `(tx.Sender, nef.CheckSum, manifest.Name)`, allocates
    /// the next contract id, writes the record + big-endian id index, runs the
    /// `_deploy` callback, emits `Deploy`, and returns the new `ContractState`.
    pub(super) fn deploy(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Post-Aspidochelone the caller must hold CallFlags.All.
        self.require_call_flags_all(engine, "Deploy")?;
        // C#: `engine.ScriptContainer is not Transaction tx` -> throw; the sender
        // is the transaction's first signer.
        let sender = engine
            .script_container()
            .and_then(|container| container.as_any().downcast_ref::<Transaction>())
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::deploy requires a transaction container",
                )
            })?
            .sender()
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::deploy: transaction has no sender",
                )
            })?;
        let nef_bytes = args.first().ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::deploy requires a NEF file")
        })?;
        let manifest_bytes = args.get(1).ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::deploy requires a manifest")
        })?;
        if nef_bytes.is_empty() {
            return Err(CoreError::invalid_operation(
                "ContractManagement::deploy: NEF file length cannot be zero",
            ));
        }
        if manifest_bytes.is_empty() {
            return Err(CoreError::invalid_operation(
                "ContractManagement::deploy: manifest length cannot be zero",
            ));
        }
        let data = self.optional_data_arg(engine, args, "deploy")?;

        // C#: AddFee(max(StoragePrice * (nef + manifest), GetMinimumDeploymentFee)
        // * FeeFactor) — the FeeFactor multiplication is the datoshi -> picoGAS
        // conversion that `charge_execution_fee` (datoshi in) performs internally.
        let snapshot = engine.snapshot_cache();
        let payload_len = i64::try_from(nef_bytes.len() + manifest_bytes.len())
            .map_err(|_| CoreError::invalid_operation("deploy payload length overflow"))?;
        let storage_component = i64::from(engine.storage_price())
            .checked_mul(payload_len)
            .ok_or_else(|| CoreError::invalid_operation("deploy storage fee overflow"))?;
        let minimum_fee = self.read_minimum_deployment_fee(&snapshot)?;
        let fee = storage_component.max(minimum_fee);
        engine.charge_execution_fee(u64::try_from(fee).unwrap_or(0))?;

        let nef = Self::parse_nef_checked(nef_bytes, "deploy")?;
        let manifest = Self::parse_manifest_checked(manifest_bytes, "deploy")?;
        // C#: Helper.Check(new Script(nef.Script, HF_Basilisk), manifest.Abi).
        Self::check_script_against_abi(
            &nef.script,
            &manifest.abi,
            engine.is_hardfork_enabled(Hardfork::HfBasilisk),
        )?;
        let hash = Helper::get_contract_hash(&sender, nef.checksum, &manifest.name);

        // C#: Policy.IsBlocked(snapshot, hash) -> "The contract {hash} has been blocked."
        if snapshot
            .get(&crate::PolicyContract::blocked_account_key(&hash))
            .is_some()
        {
            return Err(CoreError::invalid_operation(format!(
                "The contract {hash} has been blocked."
            )));
        }
        let record_key = Self::contract_storage_key(&hash);
        if snapshot.get(&record_key).is_some() {
            return Err(CoreError::invalid_operation(format!(
                "Contract Already Exists: {hash}"
            )));
        }

        let mut contract =
            ContractState::new(self.get_next_available_id(&snapshot)?, hash, nef, manifest);
        contract.update_counter = 0;
        let limits = *engine.execution_limits();
        if !Self::manifest_is_valid(&contract.manifest, &limits, &hash) {
            return Err(CoreError::invalid_operation(format!(
                "Invalid Manifest: {hash}"
            )));
        }

        // The per-contract record plus the big-endian id -> hash index entry.
        snapshot.add(
            record_key,
            StorageItem::from_bytes(Self::serialize_contract_record(&contract)?),
        );
        snapshot.add(
            Self::contract_id_storage_key(contract.id),
            StorageItem::from_bytes(hash.to_bytes().to_vec()),
        );

        self.on_deploy(engine, &contract, data, false)?;

        Self::contract_state_to_bytes(&contract, "deploy")
    }

    /// C# `ContractManagement.Update(engine, nefFile, manifest, data)` (~312-376):
    /// the CALLING contract updates itself — at least one of `nefFile` /
    /// `manifest` non-null (nullability via the dispatcher's null mask), the
    /// storage fee charged on the payload, the record re-validated
    /// (`Helper.Check` over the final NEF + manifest, name immutable,
    /// `UpdateCounter` capped at u16::MAX and bumped), `Policy.CleanWhitelist`
    /// run, then the `_deploy(data, true)` callback and the `Update` event.
    pub(super) fn update(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Post-Aspidochelone the caller must hold CallFlags.All.
        self.require_call_flags_all(engine, "Update")?;
        let nef_is_null = self.native_arg_is_null(engine, 0);
        let manifest_is_null = self.native_arg_is_null(engine, 1);
        if nef_is_null && manifest_is_null {
            return Err(CoreError::invalid_operation(
                "ContractManagement::update: NEF file and manifest cannot both be null",
            ));
        }
        let nef_bytes = if nef_is_null {
            None
        } else {
            Some(args.first().ok_or_else(|| {
                CoreError::invalid_operation("ContractManagement::update requires a NEF file arg")
            })?)
        };
        let manifest_bytes = if manifest_is_null {
            None
        } else {
            Some(args.get(1).ok_or_else(|| {
                CoreError::invalid_operation("ContractManagement::update requires a manifest arg")
            })?)
        };
        let data = self.optional_data_arg(engine, args, "update")?;

        // C#: AddFee(StoragePrice * FeeFactor * (nef?.len + manifest?.len)) — no
        // minimum-deployment-fee floor for updates.
        let payload_len =
            i64::try_from(nef_bytes.map_or(0, |b| b.len()) + manifest_bytes.map_or(0, |b| b.len()))
                .map_err(|_| CoreError::invalid_operation("update payload length overflow"))?;
        let fee = i64::from(engine.storage_price())
            .checked_mul(payload_len)
            .ok_or_else(|| CoreError::invalid_operation("update storage fee overflow"))?;
        engine.charge_execution_fee(u64::try_from(fee).unwrap_or(0))?;

        // C#: GetAndChange(Prefix_Contract ++ engine.CallingScriptHash) -> the
        // calling contract's record must exist.
        let calling_hash = engine.get_calling_script_hash().ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::update requires a calling contract")
        })?;
        let snapshot = engine.snapshot_cache();
        let mut contract =
            ContractManagement::get_contract_from_snapshot(&snapshot, &calling_hash)?.ok_or_else(
                || {
                    CoreError::invalid_operation(format!(
                        "Updating Contract Does Not Exist: {calling_hash}"
                    ))
                },
            )?;
        if contract.update_counter == u16::MAX {
            return Err(CoreError::invalid_operation(
                "The contract reached the maximum number of updates.",
            ));
        }

        if let Some(bytes) = nef_bytes {
            contract.nef = Self::parse_nef_checked(bytes, "update")?;
        }
        // C#: Policy.CleanWhitelist(engine, contract) — unconditionally, between
        // the NEF and manifest swaps.
        self.policy_clean_whitelist(engine, &contract)?;
        if let Some(bytes) = manifest_bytes {
            let new_manifest = Self::parse_manifest_checked(bytes, "update")?;
            if new_manifest.name != contract.manifest.name {
                return Err(CoreError::invalid_operation(
                    "The name of the contract can't be changed.",
                ));
            }
            let limits = *engine.execution_limits();
            if !Self::manifest_is_valid(&new_manifest, &limits, &contract.hash) {
                return Err(CoreError::invalid_operation(format!(
                    "Invalid Manifest: {}",
                    contract.hash
                )));
            }
            contract.manifest = new_manifest;
        }
        // C#: Helper.Check over the FINAL nef + manifest combination.
        Self::check_script_against_abi(
            &contract.nef.script,
            &contract.manifest.abi,
            engine.is_hardfork_enabled(Hardfork::HfBasilisk),
        )?;
        contract.update_counter += 1;

        // Persist the updated record (id, hash, and the id index are unchanged)
        // before the queued `_deploy` callback resolves the contract from storage.
        snapshot.update(
            Self::contract_storage_key(&contract.hash),
            StorageItem::from_bytes(Self::serialize_contract_record(&contract)?),
        );

        self.on_deploy(engine, &contract, data, true)?;

        Ok(Vec::new())
    }

    /// C# `contract.InitializeAsync(engine, hardfork)` dispatch for a NON-`ActiveIn`
    /// hardfork scheduled at the persisting block. Audit of every C# native
    /// `InitializeAsync` override (ContractManagement.cs:53, GasToken.cs:29,
    /// NeoToken.cs:106, OracleContract.cs:73, Notary.cs:52, PolicyContract.cs:137):
    /// only `PolicyContract` carries branches for hardforks other than its
    /// `ActiveIn` (the HF_Echidna and HF_Faun re-initializations) — every other
    /// initializer is `if (hardfork == ActiveIn)`-gated, making the non-`ActiveIn`
    /// calls no-ops.
    pub(super) fn initialize_native_for_hardfork(
        &self,
        engine: &mut ApplicationEngine,
        contract: &dyn NativeContract,
        hardfork: Hardfork,
    ) -> CoreResult<()> {
        if contract.id() == crate::PolicyContract::ID {
            return crate::PolicyContract::new().initialize_for_hardfork(engine, hardfork);
        }
        Ok(())
    }
}
