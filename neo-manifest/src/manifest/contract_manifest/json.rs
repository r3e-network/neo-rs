//! JSON parsing and JSON-specific consistency checks for `ContractManifest`.

use std::collections::{HashMap, HashSet};

use neo_error::{CoreError, CoreResult};
use serde_json::Value;

use crate::manifest::{
    ContractAbi, ContractGroup, ContractManifest, ContractPermission, ContractPermissionDescriptor,
    WildCardContainer,
};

use super::MAX_MANIFEST_LENGTH;

impl ContractManifest {
    /// Creates a manifest from a JSON string.
    pub fn from_json_str(json: &str) -> CoreResult<Self> {
        if json.len() > MAX_MANIFEST_LENGTH {
            return Err(CoreError::invalid_data(format!(
                "JSON content length {} exceeds maximum allowed size of {} bytes",
                json.len(),
                MAX_MANIFEST_LENGTH
            )));
        }
        let value: Value =
            serde_json::from_str(json).map_err(|e| CoreError::serialization(e.to_string()))?;
        Self::from_json_value(&value)
    }

    /// Alias to maintain backwards compatibility with older code paths.
    pub fn from_json(json: &str) -> CoreResult<Self> {
        Self::from_json_str(json)
    }

    /// Parses a contract manifest from JSON.
    /// This is an alias for `from_json_str` to match C# `ContractManifest.Parse` exactly.
    pub fn parse(json: &str) -> CoreResult<Self> {
        Self::from_json_str(json)
    }

    fn from_json_value(json: &Value) -> CoreResult<Self> {
        let obj = json
            .as_object()
            .ok_or_else(|| CoreError::other("Expected manifest object"))?;

        let name = obj
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| CoreError::other("Missing name"))?
            .to_string();
        if name.is_empty() {
            return Err(CoreError::other("Name in ContractManifest is empty"));
        }

        let groups = match obj.get("groups") {
            Some(Value::Array(groups)) => groups
                .iter()
                .map(ContractGroup::from_json)
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => return Err(CoreError::other("ContractManifest groups must be an array")),
            None => Vec::new(),
        };

        let features = obj
            .get("features")
            .and_then(Value::as_object)
            .ok_or_else(|| CoreError::other("Features field must be empty"))?;
        if !features.is_empty() {
            return Err(CoreError::other("Features field must be empty"));
        }

        let supported_standards = match obj.get("supportedstandards") {
            Some(Value::Array(standards)) => standards
                .iter()
                .map(|standard| {
                    let value = standard.as_str().ok_or_else(|| {
                        CoreError::other("SupportedStandards in ContractManifest must be strings")
                    })?;
                    if value.is_empty() {
                        return Err(CoreError::other(
                            "SupportedStandards in ContractManifest has empty string",
                        ));
                    }
                    Ok(value.to_string())
                })
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => {
                return Err(CoreError::other(
                    "SupportedStandards in ContractManifest must be an array",
                ));
            }
            None => Vec::new(),
        };

        let abi = ContractAbi::from_json(
            obj.get("abi")
                .ok_or_else(|| CoreError::other("Missing abi"))?,
        )?;

        let permissions = match obj.get("permissions") {
            Some(Value::Array(permissions)) => permissions
                .iter()
                .map(ContractPermission::from_json)
                .collect::<CoreResult<Vec<_>>>()?,
            Some(_) => {
                return Err(CoreError::other(
                    "ContractManifest permissions must be an array",
                ));
            }
            None => Vec::new(),
        };

        let trusts = WildCardContainer::<ContractPermissionDescriptor>::from_json(
            obj.get("trusts")
                .ok_or_else(|| CoreError::other("Missing trusts"))?,
        )?;

        let extra = match obj.get("extra") {
            None | Some(Value::Null) => None,
            Some(value @ Value::Object(_)) => Some(value.clone()),
            Some(_) => return Err(CoreError::other("ContractManifest extra must be an object")),
        };

        let manifest = Self {
            name,
            groups,
            features: HashMap::new(),
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        };

        manifest.validate_manifest_json_uniqueness()?;
        Ok(manifest)
    }

    fn validate_manifest_json_uniqueness(&self) -> CoreResult<()> {
        let mut group_keys = Vec::new();
        for group in &self.groups {
            if group_keys.iter().any(|key| key == &group.pub_key) {
                return Err(CoreError::other("Duplicate group public key in manifest"));
            }
            group_keys.push(group.pub_key.clone());
        }

        let mut standards = HashSet::new();
        for standard in &self.supported_standards {
            if !standards.insert(standard) {
                return Err(CoreError::other("Supported standards must be unique"));
            }
        }

        let mut permission_contracts = Vec::new();
        for permission in &self.permissions {
            if permission_contracts
                .iter()
                .any(|contract| contract == &permission.contract)
            {
                return Err(CoreError::other(
                    "Duplicate permission contract in manifest",
                ));
            }
            permission_contracts.push(permission.contract.clone());
        }

        let mut trusts = Vec::new();
        if let WildCardContainer::List(items) = &self.trusts {
            for item in items {
                if trusts.iter().any(|existing| existing == item) {
                    return Err(CoreError::other("Duplicate trust entry in manifest"));
                }
                trusts.push(item.clone());
            }
        }

        Ok(())
    }
}
