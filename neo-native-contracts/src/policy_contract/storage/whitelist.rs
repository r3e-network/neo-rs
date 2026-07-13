use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::StackValue;

use super::PolicyContract;
use crate::policy_contract::PREFIX_WHITELISTED_FEE_CONTRACTS;

impl PolicyContract {
    /// The whitelisted-fee storage key `(PolicyContract.ID,
    /// [Prefix_WhitelistedFeeContracts, contractHash, methodOffset])` — the C#
    /// `CreateStorageKey(Prefix_WhitelistedFeeContracts, contractHash,
    /// methodDescriptor.Offset)`, whose trailing `int` is big-endian (KeyBuilder
    /// `AddBigEndian(int)`).
    pub(crate) fn whitelist_fee_key(contract_hash: &UInt160, method_offset: i32) -> StorageKey {
        crate::keys::prefixed_hash160_i32_be_key(
            Self::ID,
            PREFIX_WHITELISTED_FEE_CONTRACTS,
            contract_hash,
            method_offset,
        )
    }

    /// The whitelisted-fee contract prefix key `(PolicyContract.ID,
    /// [Prefix_WhitelistedFeeContracts, contractHash])`.
    pub(crate) fn whitelist_contract_prefix_key(contract_hash: &UInt160) -> StorageKey {
        crate::keys::prefixed_hash160_key(Self::ID, PREFIX_WHITELISTED_FEE_CONTRACTS, contract_hash)
    }

    /// The whitelisted-fee prefix key
    /// `(PolicyContract.ID, [Prefix_WhitelistedFeeContracts])`.
    pub(super) fn whitelist_fee_prefix_key() -> StorageKey {
        crate::keys::prefixed_key(Self::ID, PREFIX_WHITELISTED_FEE_CONTRACTS, &[])
    }

    /// Decodes a stored `WhitelistedContract` struct into its fields.
    pub(in crate::policy_contract) fn decode_whitelisted_contract(
        value: &[u8],
    ) -> CoreResult<WhitelistedContractView> {
        let decoded = crate::support::codec::decode_stack_value(value, "whitelisted contract")?;
        WhitelistedContractView::from_stack_value(decoded)
    }

    /// Encodes a `WhitelistedContract` (`Struct[ContractHash, Method, ArgCount,
    /// FixedFee]`, C# `WhitelistedContract.ToStackItem`) — the write counterpart of
    /// [`decode_whitelisted_contract`].
    pub(in crate::policy_contract) fn encode_whitelisted_contract(
        view: &WhitelistedContractView,
    ) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(view, "whitelisted contract")
    }

    /// Collects the `Prefix_WhitelistedFeeContracts` storage entries in
    /// forward-seek order, the backing set for the `getWhitelistFeeContracts`
    /// iterator (C# `GetWhitelistFeeContracts`).
    pub(in crate::policy_contract) fn whitelist_fee_entries(
        &self,
        snapshot: &DataCache<impl neo_storage::CacheRead>,
    ) -> Vec<(StorageKey, StorageItem)> {
        let prefix_key = Self::whitelist_fee_prefix_key();
        snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .collect()
    }

    /// Resolves the manifest method `(name, argCount)` of a deployed contract to
    /// its bytecode offset, the discriminant of the whitelist storage key. Mirrors
    /// the shared prologue of C# `SetWhitelistFeeContract` /
    /// `RemoveWhitelistFeeContract`: `ContractManagement.GetContract` (fault
    /// "Is not a valid contract" when missing) then
    /// `Manifest.Abi.Methods.SingleOrDefault(name, argCount)` (fault when missing
    /// or ambiguous — C# `SingleOrDefault` throws on multiple matches).
    pub(in crate::policy_contract) fn resolve_whitelist_method_offset(
        &self,
        snapshot: &DataCache<impl neo_storage::CacheRead>,
        contract_hash: &UInt160,
        method: &str,
        arg_count: i32,
    ) -> CoreResult<i32> {
        let contract =
            crate::ContractManagement::get_contract_from_snapshot(snapshot, contract_hash)?
                .ok_or_else(|| CoreError::invalid_operation("Is not a valid contract"))?;
        let arg_count = usize::try_from(arg_count).map_err(|_| {
            CoreError::invalid_operation(format!(
                "Method {method} with {arg_count} args was not found in {contract_hash}"
            ))
        })?;
        let mut matches = contract
            .manifest
            .abi
            .methods
            .iter()
            .filter(|m| m.name == method && m.parameters.len() == arg_count);
        let Some(descriptor) = matches.next() else {
            return Err(CoreError::invalid_operation(format!(
                "Method {method} with {arg_count} args was not found in {contract_hash}"
            )));
        };
        if matches.next().is_some() {
            // C# SingleOrDefault throws InvalidOperationException on >1 match.
            return Err(CoreError::invalid_operation(format!(
                "Method {method} with {arg_count} args is ambiguous in {contract_hash}"
            )));
        }
        Ok(descriptor.offset)
    }
}

/// Decoded view of a stored `WhitelistedContract` (C#
/// `Struct[ContractHash, Method, ArgCount, FixedFee]`,
/// `WhitelistedContract.FromStackItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::policy_contract) struct WhitelistedContractView {
    pub(in crate::policy_contract) contract_hash: UInt160,
    pub(in crate::policy_contract) method: String,
    pub(in crate::policy_contract) arg_count: i32,
    pub(in crate::policy_contract) fixed_fee: i64,
}

impl WhitelistedContractView {
    pub(super) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let decoder =
            crate::support::codec::StructDecoder::new(&stack_value, "whitelisted contract")?;
        let contract_hash = decoder.hash160(0, "hash")?;
        let method = decoder.string(1, "method")?;
        let arg_count = decoder.i32(2, "argCount")?;
        let fixed_fee = decoder.i64(3, "fixedFee")?;
        Ok(Self {
            contract_hash,
            method,
            arg_count,
            fixed_fee,
        })
    }

    pub(super) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::ByteString(self.contract_hash.to_bytes()),
                StackValue::ByteString(self.method.as_bytes().to_vec()),
                StackValue::Integer(i64::from(self.arg_count)),
                StackValue::Integer(self.fixed_fee),
            ],
        )
    }
}

neo_vm::impl_interoperable_via_stack_value!(WhitelistedContractView);
