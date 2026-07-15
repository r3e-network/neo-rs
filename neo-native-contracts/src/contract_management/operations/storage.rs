//! ContractManagement storage keys, query helpers, and setting readers.

use super::super::{
    ContractManagement, PREFIX_CONTRACT, PREFIX_CONTRACT_HASH, PREFIX_MINIMUM_DEPLOYMENT_FEE,
    PREFIX_NEXT_AVAILABLE_ID,
};
use crate::policy_contract::POLICY_WHITELIST_FEE_CHANGED_EVENT;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, ContractState};
use neo_primitives::UInt160;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{CacheRead, StorageItem, StorageKey};
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

impl ContractManagement {
    /// Looks up a deployed contract by its script hash.
    ///
    /// Reads the per-contract record (`prefix 8` + `hash.to_bytes()`)
    /// previously written by `ContractManagement.Deploy` /
    /// `ContractManagement.Update` in the blockchain service.
    pub fn get_contract_from_snapshot<B: CacheRead>(
        snapshot: &DataCache<B>,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        let key = Self::contract_storage_key(hash);
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };

        let bytes = item.value_bytes().into_owned();
        if bytes.is_empty() {
            return Ok(None);
        }

        let state = ContractState::deserialize_contract_record(&bytes).map_err(|e| {
            CoreError::deserialization(format!("Failed to deserialize contract state: {e}"))
        })?;
        Ok(Some(state))
    }

    /// Looks up a deployed contract by its contract id.
    ///
    /// Reads the contract-id -> hash index (`prefix 12` + `id_be_bytes`)
    /// then dereferences the resulting hash via `get_contract_from_snapshot`.
    pub fn get_contract_by_id_from_snapshot<B: CacheRead>(
        snapshot: &DataCache<B>,
        id: i32,
    ) -> CoreResult<Option<ContractState>> {
        let id_key = Self::contract_id_storage_key(id);
        let Some(item) = snapshot.get(&id_key) else {
            return Ok(None);
        };
        let hash_bytes = item.value_bytes().into_owned();

        if hash_bytes.is_empty() {
            return Ok(None);
        }

        let hash = UInt160::from_bytes(&hash_bytes)
            .map_err(|e| CoreError::invalid_data(format!("Invalid contract hash bytes: {e}")))?;
        Self::get_contract_from_snapshot(snapshot, &hash)
    }

    /// Checks whether a contract is deployed in the given snapshot.
    pub fn is_contract<B: CacheRead>(snapshot: &DataCache<B>, hash: &UInt160) -> bool {
        let key = Self::contract_storage_key(hash);
        snapshot.get(&key).is_some()
    }

    #[inline]
    pub(crate) fn contract_storage_key(hash: &UInt160) -> StorageKey {
        crate::keys::prefixed_hash160_key(Self::ID, PREFIX_CONTRACT, hash)
    }

    #[inline]
    pub(in crate::contract_management) fn contract_id_storage_key(id: i32) -> StorageKey {
        crate::keys::prefixed_i32_be_key(Self::ID, PREFIX_CONTRACT_HASH, id)
    }

    #[inline]
    pub(in crate::contract_management) fn contract_id_prefix_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_CONTRACT_HASH, &[])
    }

    #[inline]
    pub(in crate::contract_management) fn setting_key(prefix: u8) -> StorageKey {
        crate::keys::prefixed_key(Self::ID, prefix, &[])
    }

    #[inline]
    pub(in crate::contract_management) fn minimum_deployment_fee_key() -> StorageKey {
        Self::setting_key(PREFIX_MINIMUM_DEPLOYMENT_FEE)
    }

    #[inline]
    pub(in crate::contract_management) fn next_available_id_key() -> StorageKey {
        Self::setting_key(PREFIX_NEXT_AVAILABLE_ID)
    }

    /// Parses the leading `Hash160` argument shared by `getContract`/`isContract`.
    pub(in crate::contract_management) fn parse_hash_arg(
        args: &[Vec<u8>],
        method: &str,
    ) -> CoreResult<UInt160> {
        crate::args::raw_account(args, &format!("ContractManagement::{method}"))
    }

    /// C# `ContractAbi.GetMethod(name, pcount) != null`: true when the manifest ABI
    /// declares a method named `name` whose parameter count matches `pcount`, where
    /// `pcount == -1` matches any count.
    pub(in crate::contract_management) fn abi_has_method(
        manifest: &neo_manifest::ContractManifest,
        name: &str,
        pcount: i32,
    ) -> bool {
        manifest
            .abi
            .methods
            .iter()
            .any(|m| m.name == name && (pcount == -1 || m.parameters.len() as i32 == pcount))
    }

    /// Marshals a `ContractState` to the Array return bytes (C# `ToStackItem` +
    /// `BinarySerializer`) via the canonical `StackItem` projection — shared by
    /// `getContract` / `getContractById`. A miss is the caller's responsibility
    /// (an empty payload encodes the C# `null`).
    pub(in crate::contract_management) fn contract_state_to_bytes(
        state: &ContractState,
        method: &str,
    ) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(
            state,
            &format!("ContractManagement::{method}"),
        )
    }

    /// Collects the `Prefix_ContractHash` storage entries (`id -> hash`) in
    /// forward-seek order, the backing set for C# `GetContractHashes`'s iterator.
    ///
    /// C# reads the contract id back out of each key
    /// (`ReadInt32BigEndian(key.Key[1..])`) and keeps only `id >= 0`, which
    /// excludes the native contracts (negative ids; their big-endian
    /// two's-complement keys sort after every non-negative id).
    pub(in crate::contract_management) fn contract_hash_entries<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> Vec<(StorageKey, StorageItem)> {
        let prefix_key = Self::contract_id_prefix_key();
        snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .filter(|(key, _)| {
                let suffix = key.suffix();
                suffix.len() >= 5
                    && i32::from_be_bytes([suffix[1], suffix[2], suffix[3], suffix[4]]) >= 0
            })
            .collect()
    }

    /// C# `PolicyContract.CleanWhitelist(engine, contract)` (PolicyContract.cs
    /// ~368), invoked cross-natively by `ContractManagement.Destroy`: deletes every
    /// `Prefix_WhitelistedFeeContracts ++ contract.Hash` entry and emits Policy's
    /// `WhitelistFeeChanged` event (`[hash, method, argCount, null]`) per removal.
    /// Entries decode as the C# `WhitelistedContract` interoperable
    /// `Struct[ContractHash, Method, ArgCount, FixedFee]`.
    pub(in crate::contract_management) fn policy_clean_whitelist<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
        contract: &ContractState,
    ) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        let prefix_key = crate::PolicyContract::whitelist_contract_prefix_key(&contract.hash);
        let entries: Vec<(StorageKey, StorageItem)> = snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .collect();
        for (key, item) in entries {
            snapshot.delete(&key);
            let decoded = crate::support::codec::decode_stack_item(
                &item.value_bytes(),
                "ContractManagement::destroy: whitelist entry",
            )?;
            let decoder =
                crate::support::codec::StructDecoder::new(&decoded, "whitelisted-contract entry")?;
            if decoder.len() < 4 {
                return Err(CoreError::invalid_data(
                    "whitelisted-contract entry must have 4 fields",
                ));
            }
            let _hash = decoder.hash160(0, "hash")?;
            let method = decoder.string(1, "method")?;
            let arg_count = decoder.i32(2, "argCount")?;
            let _fixed_fee = decoder.i64(3, "fixedFee")?;
            engine
                .send_notification(
                    crate::PolicyContract::script_hash(),
                    POLICY_WHITELIST_FEE_CHANGED_EVENT.to_owned(),
                    vec![
                        StackItem::from_byte_string(contract.hash.to_bytes()),
                        StackItem::from_byte_string(method.into_bytes()),
                        StackItem::from_int(arg_count),
                        StackItem::Null,
                    ],
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "ContractManagement::destroy: notify: {e}"
                    ))
                })?;
        }
        Ok(())
    }

    pub(in crate::contract_management) fn read_required_i64_setting<B: CacheRead>(
        snapshot: &DataCache<B>,
        key: StorageKey,
        setting: &str,
    ) -> CoreResult<i64> {
        let Some(item) = snapshot.get(&key) else {
            return Err(CoreError::invalid_data(format!(
                "ContractManagement {setting} is missing"
            )));
        };
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| {
                CoreError::invalid_operation(format!("ContractManagement {setting} out of range"))
            })
    }

    pub(in crate::contract_management) fn read_minimum_deployment_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<i64> {
        Self::read_required_i64_setting(
            snapshot,
            Self::minimum_deployment_fee_key(),
            "MinimumDeploymentFee",
        )
    }

    pub(in crate::contract_management) fn read_next_available_id<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<i64> {
        Self::read_required_i64_setting(snapshot, Self::next_available_id_key(), "NextAvailableId")
    }

    /// C# `SetMinimumDeploymentFee` storage effect: overwrite
    /// `Prefix_MinimumDeploymentFee` (`GetAndChange(...).Set(value)`). The key is
    /// genesis-initialised, so absence faults; the value is stored as the full
    /// signed-LE BigInteger (the C# parameter is `BigInteger`, not `long`).
    pub(in crate::contract_management) fn put_minimum_deployment_fee<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        value: &BigInt,
    ) -> CoreResult<()> {
        let key = Self::minimum_deployment_fee_key();
        if snapshot.get(&key).is_none() {
            return Err(CoreError::invalid_data(
                "ContractManagement MinimumDeploymentFee is missing",
            ));
        }
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
        Ok(())
    }

    /// C# `ContractManagement.GetNextAvailableId`: returns the current
    /// `Prefix_NextAvailableId` value and stores `value + 1`
    /// (`item.Add(1)`). The key is genesis-initialised to 1; absence faults.
    pub(in crate::contract_management) fn get_next_available_id<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<i32> {
        let value = self.read_next_available_id(snapshot)?;
        let id = i32::try_from(value).map_err(|_| {
            // C# casts `(int)(BigInteger)item`, which throws on overflow.
            CoreError::invalid_operation("next available contract id out of range")
        })?;
        snapshot.update(
            Self::next_available_id_key(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                i64::from(id) + 1,
            ))),
        );
        Ok(id)
    }
}
