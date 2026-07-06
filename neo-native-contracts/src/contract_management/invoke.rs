//! ContractManagement native-method handlers.
//!
//! Keeps query, settings, deploy/update, and destroy method bodies out of the
//! contract root while preserving storage layout, committee checks, lifecycle
//! operation handlers, and hardfork-gated destroy ordering. Dispatch is declared
//! by the metadata binding table and `native_contract_dispatch!`.

use super::{CONTRACT_DESTROY_EVENT, ContractManagement};
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_primitives::FindOptions;
use neo_storage::StorageKey;
use neo_storage::persistence::SeekDirection;
use neo_vm::StackItem;
use num_bigint::BigInt;

impl ContractManagement {
    pub(super) fn invoke_get_contract(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let hash = Self::parse_hash_arg(args, "getContract")?;
        // C# `GetContract` returns the ContractState (as an Array via
        // ToStackItem) or null on miss; the native return marshaling
        // encodes a null Array result as an empty payload.
        match Self::get_contract_from_snapshot(&snapshot, &hash)? {
            Some(state) => Self::contract_state_to_bytes(&state, "getContract"),
            None => Ok(Vec::new()),
        }
    }

    pub(super) fn invoke_get_contract_by_id(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C# `GetContractById` maps the id to a hash via the
        // contract-id index, then returns that ContractState (or null).
        let id = crate::args::raw_i32_arg(args, 0, "ContractManagement::getContractById")?;
        match Self::get_contract_by_id_from_snapshot(&snapshot, id)? {
            Some(state) => Self::contract_state_to_bytes(&state, "getContractById"),
            None => Ok(Vec::new()),
        }
    }

    pub(super) fn invoke_get_minimum_deployment_fee(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let fee = self.read_minimum_deployment_fee(&snapshot)?;
        Ok(BigInt::from(fee).to_signed_bytes_le())
    }

    pub(super) fn invoke_set_minimum_deployment_fee(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
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

    pub(super) fn invoke_get_contract_hashes(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C# GetContractHashes: an iterator over Prefix_ContractHash with
        // FindOptions.RemovePrefix and prefix length 1, yielding
        // Struct[id_bytes, hash]. The 4-byte iterator id is decoded back
        // into an InteropInterface (StorageIterator) by the dispatcher.
        let results = self.contract_hash_entries(&snapshot);
        let iterator_id = engine
            .create_storage_iterator_with_options(results, 1, FindOptions::RemovePrefix)
            .map_err(|e| {
                CoreError::invalid_operation(format!("ContractManagement::getContractHashes: {e}"))
            })?;
        Ok(iterator_id.to_le_bytes().to_vec())
    }

    pub(super) fn invoke_is_contract(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let hash = Self::parse_hash_arg(args, "isContract")?;
        // C# `IsContract` = snapshot.Contains(key(Prefix_Contract, hash)).
        Ok(vec![u8::from(Self::is_contract(&snapshot, &hash))])
    }

    pub(super) fn invoke_has_method(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let hash = Self::parse_hash_arg(args, "hasMethod")?;
        let method_name =
            crate::args::raw_string_arg(args, 1, "ContractManagement::hasMethod", "method name")?;
        let pcount = crate::args::raw_i32_arg(args, 2, "ContractManagement::hasMethod")?;
        // C#: false if the contract does not exist; otherwise whether its
        // manifest ABI declares the (method, pcount) method.
        let has = match Self::get_contract_from_snapshot(&snapshot, &hash)? {
            Some(state) => Self::abi_has_method(&state.manifest, &method_name, pcount),
            None => false,
        };
        Ok(vec![u8::from(has)])
    }

    pub(super) fn invoke_deploy(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Both deploy overloads land here; args.len() (2 vs 3) selects
        // the C# overload - the 2-arg form forwards data = StackItem.Null.
        self.deploy(engine, args)
    }

    pub(super) fn invoke_update(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Both update overloads land here, same overload convention.
        self.update(engine, args)
    }

    pub(super) fn invoke_destroy(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C# Destroy (~382): the CALLING contract destroys itself
        // (hash = engine.CallingScriptHash; a missing calling context
        // is the C# null-deref fault).
        let hash = engine.get_calling_script_hash().ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::destroy requires a calling contract")
        })?;
        // C#: `if (contract is null) return;` - a non-contract caller
        // is a successful no-op.
        let Some(contract) = Self::get_contract_from_snapshot(&snapshot, &hash)? else {
            return Ok(Vec::new());
        };
        // C# `DestroyInternal(engine, blockBeforeErase)`: HF_Gorgon's
        // `destroy` (V1) blocks the account + cleans the whitelist BEFORE
        // erasing the contract (so a votes-revoke can still claim GAS while
        // the contract is in the snapshot); pre-Gorgon (V0) does it AFTER
        // (skipping side calls since the contract is already removed). The
        // ops are identical - only the order relative to the deletes flips.
        let block_before_erase = engine.is_hardfork_enabled(Hardfork::HfGorgon);
        if block_before_erase {
            crate::PolicyContract::new().block_account_internal(engine, &hash)?;
            self.policy_clean_whitelist(engine, &contract)?;
        }
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
        // Pre-Gorgon path: `await Policy.BlockAccountInternal(engine, hash)`
        // then `Policy.CleanWhitelist(engine, contract)` AFTER the erase.
        if !block_before_erase {
            crate::PolicyContract::new().block_account_internal(engine, &hash)?;
            self.policy_clean_whitelist(engine, &contract)?;
        }
        // Emit the Destroy event with the destroyed hash.
        engine
            .send_notification(
                Self::script_hash(),
                CONTRACT_DESTROY_EVENT.to_owned(),
                vec![StackItem::from_byte_string(hash.to_bytes())],
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("ContractManagement::destroy: notify: {e}"))
            })?;
        Ok(Vec::new())
    }
}
