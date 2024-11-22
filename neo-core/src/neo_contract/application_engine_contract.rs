use std::io::Error;
use crate::cryptography::ECPoint;
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::call_flags::CallFlags;
use crate::neo_contract::contract_parameter_type::ContractParameterType;
use crate::neo_contract::execution_context_state::ExecutionContextState;
use crate::neo_contract::interop_descriptor::InteropDescriptor;
use crate::neo_contract::trigger_type::TriggerType;
use neo_type::H160;

impl ApplicationEngine {
    /// The `InteropDescriptor` of System.Contract.Call.
    /// Use it to call another contract dynamically.
    pub const SYSTEM_CONTRACT_CALL: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.Call",
        ApplicationEngine::call_contract,
        1 << 15,
        CallFlags::READ_STATES | CallFlags::ALLOW_CALL
    );

    /// The `InteropDescriptor` of System.Contract.CallNative.
    /// Note: It is for internal use only. Do not use it directly in smart contracts.
    pub const SYSTEM_CONTRACT_CALL_NATIVE: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.CallNative",
        ApplicationEngine::call_native_contract,
        0,
        CallFlags::NONE
    );

    /// The `InteropDescriptor` of System.Contract.GetCallFlags.
    /// Gets the `CallFlags` of the current context.
    pub const SYSTEM_CONTRACT_GET_CALL_FLAGS: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.GetCallFlags",
        ApplicationEngine::get_call_flags,
        1 << 10,
        CallFlags::NONE
    );

    /// The `InteropDescriptor` of System.Contract.CreateStandardAccount.
    /// Calculates corresponding account scripthash for the given public key.
    pub const SYSTEM_CONTRACT_CREATE_STANDARD_ACCOUNT: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.CreateStandardAccount",
        ApplicationEngine::create_standard_account,
        0,
        CallFlags::NONE
    );

    /// The `InteropDescriptor` of System.Contract.CreateMultisigAccount.
    /// Calculates corresponding multisig account scripthash for the given public keys.
    pub const SYSTEM_CONTRACT_CREATE_MULTISIG_ACCOUNT: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.CreateMultisigAccount",
        ApplicationEngine::create_multisig_account,
        0,
        CallFlags::NONE
    );

    /// The `InteropDescriptor` of System.Contract.NativeOnPersist.
    /// Note: It is for internal use only. Do not use it directly in smart contracts.
    pub const SYSTEM_CONTRACT_NATIVE_ON_PERSIST: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.NativeOnPersist",
        ApplicationEngine::native_on_persist_async,
        0,
        CallFlags::STATES
    );

    /// The `InteropDescriptor` of System.Contract.NativePostPersist.
    /// Note: It is for internal use only. Do not use it directly in smart contracts.
    pub const SYSTEM_CONTRACT_NATIVE_POST_PERSIST: InteropDescriptor = InteropDescriptor::new(
        "System.Contract.NativePostPersist",
        ApplicationEngine::native_post_persist_async,
        0,
        CallFlags::STATES
    );

    /// The implementation of System.Contract.Call.
    /// Use it to call another contract dynamically.
    pub fn call_contract(&mut self, contract_hash: &H160, method: &str, call_flags: CallFlags, args: Array) -> Result<(), Error> {
        if method.starts_with('_') {
            return Err(Error::new(format!("Invalid Method Name: {}", method)));
        }
        if (call_flags & !CallFlags::ALL) != CallFlags::NONE {
            return Err(Error::new("Invalid CallFlags"));
        }

        let contract = self.snapshot_cache.get_contract(&contract_hash)
            .ok_or_else(|| Error::new(format!("Called Contract Does Not Exist: {}.{}", contract_hash, method)))?;

        let md = contract.manifest.abi.get_method(method, args.len())
            .ok_or_else(|| Error::new(format!("Method \"{}\" with {} parameter(s) doesn't exist in the contract {}.", method, args.len(), contract_hash)))?;

        let has_return_value = md.return_type != ContractParameterType::Void;

        let context = self.call_contract_internal(&contract, md, call_flags, has_return_value, args)?;
        context.get_state_mut::<ExecutionContextState>().is_dynamic_call = true;

        Ok(())
    }

    /// The implementation of System.Contract.CallNative.
    /// Calls to a native contract.
    pub fn call_native_contract(&mut self, version: u8) -> Result<(), Error> {
        let contract = NativeContract::get_contract(&self.current_script_hash())
            .ok_or_else(|| Error::new("It is not allowed to use \"System.Contract.CallNative\" directly."))?;

        if !contract.is_active(&self.protocol_settings, self.snapshot_cache.current_index()) {
            return Err(Error::new(format!("The native contract {} is not active.", contract.name())));
        }

        contract.invoke(self, version)
    }

    /// The implementation of System.Contract.GetCallFlags.
    /// Gets the `CallFlags` of the current context.
    pub fn get_call_flags(&self) -> CallFlags {
        self.current_context().get_state::<ExecutionContextState>().call_flags
    }

    /// The implementation of System.Contract.CreateStandardAccount.
    /// Calculates corresponding account scripthash for the given public key.
    pub fn create_standard_account(&mut self, pub_key: &ECPoint) -> Result<H160, Error> {
        let fee = if self.is_hardfork_enabled(Hardfork::HF_Aspidochelone) {
            self.check_sig_price
        } else {
            1 << 8
        };
        self.add_fee(fee * self.exec_fee_factor)?;
        Ok(Contract::create_signature_redeem_script(pub_key).to_script_hash())
    }

    /// The implementation of System.Contract.CreateMultisigAccount.
    /// Calculates corresponding multisig account scripthash for the given public keys.
    pub fn create_multisig_account(&mut self, m: i32, pub_keys: &[ECPoint]) -> Result<H160, Error> {
        let fee = if self.is_hardfork_enabled(Hardfork::HF_Aspidochelone) {
            self.check_sig_price * pub_keys.len() as i64
        } else {
            1 << 8
        };
        self.add_fee(fee * self.exec_fee_factor)?;
        Ok(Contract::create_multi_sig_redeem_script(m, pub_keys).to_script_hash())
    }

    /// The implementation of System.Contract.NativeOnPersist.
    /// Calls to the `on_persist` of all native contracts.
    pub async fn native_on_persist_async(&mut self) -> Result<(), Error> {
        if self.trigger != TriggerType::ON_PERSIST {
            return Err(Error::new("Invalid operation"));
        }
        for contract in NativeContract::contracts() {
            if contract.is_active(&self.protocol_settings, self.persisting_block.index) {
                contract.on_persist(self).await?;
            }
        }
        Ok(())
    }

    /// The implementation of System.Contract.NativePostPersist.
    /// Calls to the `post_persist` of all native contracts.
    pub async fn native_post_persist_async(&mut self) -> Result<(), Error> {
        if self.trigger != TriggerType::POST_PERSIST {
            return Err(Error::new("Invalid operation"));
        }
        for contract in NativeContract::contracts() {
            if contract.is_active(&self.protocol_settings, self.persisting_block.index) {
                contract.post_persist(self).await?;
            }
        }
        Ok(())
    }
}
