use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm_rs::{ExecutionEngineLimits, StackValue};

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
        let limits = ExecutionEngineLimits::default();
        let decoded = BinarySerializer::deserialize_stack_value_with_limits(
            value,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("whitelisted contract: {e}")))?;
        WhitelistedContractView::from_stack_value(decoded)
    }

    /// Encodes a `WhitelistedContract` (`Struct[ContractHash, Method, ArgCount,
    /// FixedFee]`, C# `WhitelistedContract.ToStackItem`) — the write counterpart of
    /// [`decode_whitelisted_contract`].
    pub(in crate::policy_contract) fn encode_whitelisted_contract(
        view: &WhitelistedContractView,
    ) -> CoreResult<Vec<u8>> {
        let item = view.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::invalid_operation(format!("encode whitelisted contract: {e}")))
    }

    /// Collects the `Prefix_WhitelistedFeeContracts` storage entries in
    /// forward-seek order, the backing set for the `getWhitelistFeeContracts`
    /// iterator (C# `GetWhitelistFeeContracts`).
    pub(in crate::policy_contract) fn whitelist_fee_entries(
        &self,
        snapshot: &DataCache,
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
        snapshot: &DataCache,
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
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data(
                "whitelisted contract is not a struct",
            ));
        };
        let hash_bytes = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract missing hash"))?
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract hash is not byte-like"))?;
        let contract_hash =
            crate::args::bytes_to_hash160(&hash_bytes, "whitelisted contract hash")?;
        let method = items
            .get(1)
            .and_then(neo_vm_rs::stack_value_as_string)
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract method is not UTF-8"))?;
        let arg_count = items
            .get(2)
            .and_then(neo_vm_rs::stack_value_as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract argCount out of range"))?;
        let fixed_fee = items
            .get(3)
            .and_then(neo_vm_rs::stack_value_as_i64)
            .ok_or_else(|| CoreError::invalid_data("whitelisted contract fixedFee out of range"))?;
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
