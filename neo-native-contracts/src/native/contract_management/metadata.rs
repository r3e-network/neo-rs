//! ContractManagement native method and event metadata tables.
//!
//! Migrated from `neo-core/src/smart_contract/native/contract_management/metadata.rs`.
//! `crate::smart_contract::manifest::*` → `neo_smart_contract::manifest::*`; the
//! native macro/`NativeMethod` paths move from `crate::smart_contract::native::*`
//! to `crate::native::*`.

use super::ContractManagement;
use crate::native::NativeMethod;
use crate::native::metadata_macros::event_descriptor;
use crate::native::method_macros::neo_native_methods;
use neo_smart_contract::manifest::ContractEventDescriptor;

impl ContractManagement {
    pub(super) fn native_methods() -> Vec<NativeMethod> {
        neo_native_methods![
            safe "getContract", fee = 1 << 15, flags = [READ_STATES], params = [Hash160], returns = Array, names = ["hash"];
            unsafe "deploy", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray], returns = Array, names = ["nefFile", "manifest"];
            unsafe "deploy", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray, Any], returns = Array, names = ["nefFile", "manifest", "data"];
            unsafe "update", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray], returns = Void, names = ["nefFile", "manifest"];
            unsafe "update", fee = 0, flags = [STATES, ALLOW_NOTIFY], params = [ByteArray, ByteArray, Any], returns = Void, names = ["nefFile", "manifest", "data"];
            unsafe "destroy", fee = 1 << 15, flags = [STATES, ALLOW_NOTIFY], params = [], returns = Void, deprecated = HfGorgon;
            unsafe "destroy", fee = 1 << 15, flags = [STATES, ALLOW_NOTIFY], params = [], returns = Void, active = HfGorgon;
            safe "getMinimumDeploymentFee", fee = 1 << 15, flags = [READ_STATES], params = [], returns = Integer;
            unsafe "setMinimumDeploymentFee", fee = 1 << 15, flags = [STATES], params = [Integer], returns = Void, names = ["value"];
            safe "hasMethod", fee = 1 << 15, flags = [READ_STATES], params = [Hash160, String, Integer], returns = Boolean, names = ["hash", "method", "pcount"];
            safe "getContractById", fee = 1 << 15, flags = [READ_STATES], params = [Integer], returns = Array, names = ["id"];
            safe "isContract", fee = 1 << 14, flags = [READ_STATES], params = [Hash160], returns = Boolean, active = HfEchidna, names = ["hash"];
            safe "getContractHashes", fee = 1 << 15, flags = [READ_STATES], params = [], returns = InteropInterface;
        ]
    }

    pub(super) fn event_descriptors() -> Vec<ContractEventDescriptor> {
        vec![
            event_descriptor!("Deploy", ["Hash" => Hash160]),
            event_descriptor!("Update", ["Hash" => Hash160]),
            event_descriptor!("Destroy", ["Hash" => Hash160]),
        ]
    }
}
