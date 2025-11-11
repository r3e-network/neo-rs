use alloc::{collections::BTreeMap, string::String, vec::Vec};

use neo_base::{encoding::NeoEncode, hash::Hash160};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::error::ContractError;

use crate::manifest::{
    ContractAbi, ContractFeatures, ContractGroup, ContractMethod, ContractPermission,
    ContractPermissionDescriptor, WildcardContainer,
};

pub const MAX_MANIFEST_LENGTH: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractManifest {
    pub name: String,
    #[serde(default)]
    pub groups: Vec<ContractGroup>,
    pub features: ContractFeatures,
    #[serde(default, rename = "supportedstandards")]
    pub supported_standards: Vec<String>,
    pub abi: ContractAbi,
    #[serde(default)]
    pub permissions: Vec<ContractPermission>,
    #[serde(default)]
    pub trusts: WildcardContainer<ContractPermissionDescriptor>,
    #[serde(default)]
    pub extra: BTreeMap<String, JsonValue>,
}

impl ContractManifest {
    pub fn new(name: String, abi: ContractAbi) -> Self {
        Self {
            name,
            groups: Vec::new(),
            features: ContractFeatures::default(),
            supported_standards: Vec::new(),
            abi,
            permissions: vec![ContractPermission::allow_all()],
            trusts: WildcardContainer::wildcard(),
            extra: BTreeMap::new(),
        }
    }

    pub fn find_method(&self, name: &str) -> Option<&ContractMethod> {
        self.abi.methods.iter().find(|method| method.name == name)
    }

    pub fn find_method_with_arity(
        &self,
        name: &str,
        parameter_count: usize,
    ) -> Option<&ContractMethod> {
        self.abi.find_method(name, parameter_count)
    }

    pub fn allows_contract(
        &self,
        target_hash: &Hash160,
        method: &str,
        target_manifest: Option<&ContractManifest>,
    ) -> bool {
        let groups = target_manifest.map(|m| m.groups.as_slice()).unwrap_or(&[]);
        self.permissions
            .iter()
            .any(|permission| permission.allows(target_hash, method, groups))
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        if self.name.is_empty() {
            return Err(ContractError::Manifest(
                "contract name cannot be empty".into(),
            ));
        }

        if self.encoded_size() > MAX_MANIFEST_LENGTH {
            return Err(ContractError::Manifest(
                "manifest exceeds 64KiB limit".into(),
            ));
        }

        if self.permissions.is_empty() {
            return Err(ContractError::Manifest(
                "manifest must declare at least one permission".into(),
            ));
        }

        Ok(())
    }

    fn encoded_size(&self) -> usize {
        let mut buf = Vec::new();
        self.neo_encode(&mut buf);
        buf.len()
    }
}

impl Default for ContractManifest {
    fn default() -> Self {
        Self::new(String::new(), ContractAbi::default())
    }
}
