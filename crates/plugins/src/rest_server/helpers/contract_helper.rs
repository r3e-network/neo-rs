//! Rust port of `Neo.Plugins.RestServer.Helpers.ContractHelper`.
//!
//! The helper exposes routines used by the REST server controllers to inspect
//! contract manifests and validate NEP-17/NEP-11 support. Implementation
//! mirrors the C# helpers as closely as possible so behaviour stays aligned
//! with the reference plugin.

use neo_core::neo_io::MemoryReader;
use neo_core::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::smart_contract::manifest::{ContractMethodDescriptor, ContractParameterDefinition};
use neo_core::smart_contract::{ContractParameterType, ContractState, StorageItem, StorageKey};
use neo_core::UInt160;

const CONTRACT_MANAGEMENT_ID: i32 = -1;
const PREFIX_CONTRACT: u8 = 0x08;

/// Contract helper utilities matching the C# implementation.
pub struct ContractHelper;

impl ContractHelper {
    /// Gets the ABI parameters for the supplied event when the contract exists.
    pub fn get_abi_event_params<S>(
        store: &S,
        script_hash: &UInt160,
        event_name: &str,
    ) -> Option<Vec<ContractParameterDefinition>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let contract = Self::load_contract(store, script_hash).ok().flatten()?;
        contract
            .manifest
            .abi
            .events
            .iter()
            .find(|descriptor| descriptor.name.eq_ignore_ascii_case(event_name))
            .map(|descriptor| descriptor.parameters.clone())
    }

    /// Returns `true` when the specified contract hash supports NEP-17.
    pub fn is_nep17_supported<S>(store: &S, script_hash: &UInt160) -> bool
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        match Self::load_contract(store, script_hash) {
            Ok(Some(contract)) => Self::is_nep17_supported_contract(&contract),
            _ => false,
        }
    }

    /// Returns `true` when the specified contract hash supports NEP-11.
    pub fn is_nep11_supported<S>(store: &S, script_hash: &UInt160) -> bool
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        match Self::load_contract(store, script_hash) {
            Ok(Some(contract)) => Self::is_nep11_supported_contract(&contract),
            _ => false,
        }
    }

    /// Returns `true` when the provided contract satisfies the NEP-17 checks.
    pub fn is_nep17_supported_contract(contract: &ContractState) -> bool {
        let mut abi = contract.manifest.abi.clone();

        if !contract
            .manifest
            .supported_standards
            .iter()
            .any(|standard| standard.eq_ignore_ascii_case("NEP-17"))
        {
            return false;
        }

        let symbol_method = clone_method(&mut abi, "symbol", 0);
        let decimals_method = clone_method(&mut abi, "decimals", 0);
        let total_supply_method = clone_method(&mut abi, "totalSupply", 0);
        let balance_of_method = clone_method(&mut abi, "balanceOf", 1);
        let transfer_method = clone_method(&mut abi, "transfer", 4);
        let transfer_event = contract
            .manifest
            .abi
            .events
            .iter()
            .find(|event| event.name == "Transfer");

        let Some(symbol) = symbol_method else {
            return false;
        };
        let Some(decimals) = decimals_method else {
            return false;
        };
        let Some(total_supply) = total_supply_method else {
            return false;
        };
        let Some(balance_of) = balance_of_method else {
            return false;
        };
        let Some(transfer) = transfer_method else {
            return false;
        };

        let symbol_valid =
            symbol.safe && symbol.return_type == ContractParameterType::String;
        let decimals_valid =
            decimals.safe && decimals.return_type == ContractParameterType::Integer;
        let total_supply_valid =
            total_supply.safe && total_supply.return_type == ContractParameterType::Integer;
        let balance_of_valid = balance_of.safe
            && balance_of.return_type == ContractParameterType::Integer
            && balance_of
                .parameters
                .first()
                .map(|param| param.param_type == ContractParameterType::Hash160)
                .unwrap_or(false);
        let transfer_valid = !transfer.safe
            && transfer.return_type == ContractParameterType::Boolean
            && transfer.parameters.len() == 4
            && transfer.parameters[0].param_type == ContractParameterType::Hash160
            && transfer.parameters[1].param_type == ContractParameterType::Hash160
            && transfer.parameters[2].param_type == ContractParameterType::Integer
            && transfer.parameters[3].param_type == ContractParameterType::Any;

        let transfer_event_valid = transfer_event
            .map(|event| {
                event.parameters.len() == 3
                    && event.parameters[0].param_type == ContractParameterType::Hash160
                    && event.parameters[1].param_type == ContractParameterType::Hash160
                    && event.parameters[2].param_type == ContractParameterType::Integer
            })
            .unwrap_or(false);

        symbol_valid
            && decimals_valid
            && total_supply_valid
            && balance_of_valid
            && transfer_valid
            && transfer_event_valid
    }

    /// Returns `true` when the provided contract satisfies the NEP-11 checks.
    pub fn is_nep11_supported_contract(contract: &ContractState) -> bool {
        let mut abi = contract.manifest.abi.clone();

        if !contract
            .manifest
            .supported_standards
            .iter()
            .any(|standard| standard.eq_ignore_ascii_case("NEP-11"))
        {
            return false;
        }

        let symbol_method = clone_method(&mut abi, "symbol", 0);
        let decimals_method = clone_method(&mut abi, "decimals", 0);
        let total_supply_method = clone_method(&mut abi, "totalSupply", 0);
        let balance_of_method_one = clone_method(&mut abi, "balanceOf", 1);
        let balance_of_method_two = clone_method(&mut abi, "balanceOf", 2);
        let tokens_of_method = clone_method(&mut abi, "tokensOf", 1);
        let owner_of_method = clone_method(&mut abi, "ownerOf", 1);
        let transfer_method_one = clone_method(&mut abi, "transfer", 3);
        let transfer_method_two = clone_method(&mut abi, "transfer", 5);
        let transfer_event = contract
            .manifest
            .abi
            .events
            .iter()
            .find(|event| event.name == "Transfer");

        let Some(symbol) = symbol_method else {
            return false;
        };
        let Some(decimals) = decimals_method else {
            return false;
        };
        let Some(total_supply) = total_supply_method else {
            return false;
        };
        let Some(tokens_of) = tokens_of_method else {
            return false;
        };

        let symbol_valid =
            symbol.safe && symbol.return_type == ContractParameterType::String;
        let decimals_valid =
            decimals.safe && decimals.return_type == ContractParameterType::Integer;
        let total_supply_valid =
            total_supply.safe && total_supply.return_type == ContractParameterType::Integer;

        let balance_of_valid_one = balance_of_method_one
            .as_ref()
            .map(|method| {
                method.safe
                    && method.return_type == ContractParameterType::Integer
                && method.parameters.first().map_or(false, |param| {
                    param.param_type == ContractParameterType::Hash160
                })
            })
            .unwrap_or(false);

        let balance_of_valid_two = balance_of_method_two.as_ref().map(|method| {
            method.safe
                && method.return_type == ContractParameterType::Integer
                && method.parameters.len() == 2
                && method.parameters[0].param_type == ContractParameterType::Hash160
                && method.parameters[1].param_type == ContractParameterType::ByteArray
        });

        let tokens_of_valid = tokens_of.safe
            && tokens_of.return_type == ContractParameterType::InteropInterface
            && tokens_of.parameters.len() == 1
            && tokens_of.parameters[0].param_type == ContractParameterType::Hash160;

        let owner_of_valid = owner_of_method.as_ref().map(|method| {
            method.safe
                && method.parameters.len() == 1
                && method.parameters[0].param_type == ContractParameterType::ByteArray
                && (method.return_type == ContractParameterType::Hash160
                    || method.return_type == ContractParameterType::InteropInterface)
        });

        let transfer_valid_one = transfer_method_one.as_ref().map(|method| {
            !method.safe
                && method.return_type == ContractParameterType::Boolean
                && method.parameters.len() == 3
                && method.parameters[0].param_type == ContractParameterType::Hash160
                && method.parameters[1].param_type == ContractParameterType::ByteArray
                && method.parameters[2].param_type == ContractParameterType::Any
        });

        let transfer_valid_two = transfer_method_two.as_ref().map(|method| {
            !method.safe
                && method.return_type == ContractParameterType::Boolean
                && method.parameters.len() == 5
                && method.parameters[0].param_type == ContractParameterType::Hash160
                && method.parameters[1].param_type == ContractParameterType::Hash160
                && method.parameters[2].param_type == ContractParameterType::Integer
                && method.parameters[3].param_type == ContractParameterType::ByteArray
                && method.parameters[4].param_type == ContractParameterType::Any
        });

        let transfer_event_valid = transfer_event
            .map(|event| {
                event.parameters.len() == 4
                    && event.parameters[0].param_type == ContractParameterType::Hash160
                    && event.parameters[1].param_type == ContractParameterType::Hash160
                    && event.parameters[2].param_type == ContractParameterType::Integer
                    && event.parameters[3].param_type == ContractParameterType::ByteArray
            })
            .unwrap_or(false);

        symbol_valid
            && decimals_valid
            && total_supply_valid
            && (balance_of_valid_two.unwrap_or(false) || balance_of_valid_one)
            && tokens_of_valid
            && owner_of_valid.unwrap_or(false)
            && (transfer_valid_two.unwrap_or(false) || transfer_valid_one.unwrap_or(false))
            && transfer_event_valid
    }

    /// Returns a contract method descriptor when present in the manifest.
    pub fn get_contract_method<S>(
        store: &S,
        script_hash: &UInt160,
        method: &str,
        parameter_count: i32,
    ) -> Option<ContractMethodDescriptor>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let contract = Self::load_contract(store, script_hash).ok().flatten()?;
        Self::get_contract_method_from_state(&contract, method, parameter_count)
    }

    /// Helper that returns a method descriptor from a known contract state.
    pub fn get_contract_method_from_state(
        contract: &ContractState,
        method: &str,
        parameter_count: i32,
    ) -> Option<ContractMethodDescriptor> {
        let mut abi = contract.manifest.abi.clone();
        abi.get_method(method, parameter_count).cloned()
    }

    /// Retrieves the contract state for the given script hash when present.
    pub fn get_contract_state<S>(
        store: &S,
        script_hash: &UInt160,
    ) -> Result<Option<ContractState>, String>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        Self::load_contract(store, script_hash)
    }

    fn load_contract<S>(
        store: &S,
        script_hash: &UInt160,
    ) -> Result<Option<ContractState>, String>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create_with_uint160(
            CONTRACT_MANAGEMENT_ID,
            PREFIX_CONTRACT,
            script_hash,
        );

        let Some(item) = store.try_get(&key) else {
            return Ok(None);
        };

        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(None);
        }

        let mut reader = MemoryReader::new(&bytes);
        ContractState::deserialize(&mut reader)
            .map(Some)
            .map_err(|err| err.to_string())
    }

    pub fn list_contracts<S>(store: &S) -> Result<Vec<ContractState>, String>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let prefix = StorageKey::create(CONTRACT_MANAGEMENT_ID, PREFIX_CONTRACT);
        let mut contracts = Vec::new();

        for (_, item) in store.find(Some(&prefix), SeekDirection::Forward) {
            let bytes = item.get_value();
            if bytes.is_empty() {
                continue;
            }

            let mut reader = MemoryReader::new(&bytes);
            match ContractState::deserialize(&mut reader) {
                Ok(contract) => contracts.push(contract),
                Err(err) => return Err(err.to_string()),
            }
        }

        Ok(contracts)
    }
}

fn clone_method(
    abi: &mut neo_core::smart_contract::manifest::ContractAbi,
    name: &str,
    parameter_count: i32,
) -> Option<ContractMethodDescriptor> {
    abi.get_method(name, parameter_count).map(Clone::clone)
}
