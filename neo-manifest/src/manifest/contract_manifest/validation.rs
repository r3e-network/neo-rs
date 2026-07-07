//! Semantic validation for `ContractManifest`.
//!
//! JSON, binary wire, and VM-stack adapters can all construct a manifest. This
//! module keeps the post-construction invariants in one place so callers get
//! the same deployability checks regardless of the source encoding.

use std::collections::HashSet;

use neo_error::{CoreError, CoreResult};

use crate::manifest::{ContractManifest, ContractPermissionDescriptor, WildCardContainer};

use super::MAX_MANIFEST_LENGTH;

impl ContractManifest {
    /// Validates the manifest after it has been decoded or constructed.
    pub fn validate(&self) -> CoreResult<()> {
        if self.name.is_empty() {
            return Err(CoreError::invalid_data("Contract name cannot be empty"));
        }

        if !self.features.is_empty() {
            return Err(CoreError::invalid_data("Features field must be empty"));
        }

        let mut seen_standards = HashSet::new();
        for standard in &self.supported_standards {
            if standard.is_empty() {
                return Err(CoreError::invalid_data(
                    "Supported standards cannot include empty strings",
                ));
            }
            if !seen_standards.insert(standard.as_str()) {
                return Err(CoreError::invalid_data(
                    "Supported standards must be unique",
                ));
            }
        }

        if self.size() > MAX_MANIFEST_LENGTH {
            return Err(CoreError::invalid_data(
                "Manifest exceeds maximum allowed length",
            ));
        }

        let mut group_keys = Vec::new();
        for group in &self.groups {
            group.validate()?;
            if group_keys.iter().any(|key| key == &group.pub_key) {
                return Err(CoreError::invalid_data(
                    "Duplicate group public key in manifest",
                ));
            }
            group_keys.push(group.pub_key.clone());
        }

        // Neo N3 allows empty permissions arrays, which means the contract is
        // not allowed to call any external methods.
        let mut permission_contracts = Vec::new();
        for permission in &self.permissions {
            permission.validate()?;
            if permission_contracts
                .iter()
                .any(|contract| contract == &permission.contract)
            {
                return Err(CoreError::invalid_data(
                    "Duplicate permission contract in manifest",
                ));
            }
            permission_contracts.push(permission.contract.clone());
        }

        if let WildCardContainer::List(trusts) = &self.trusts {
            let mut seen_trusts = Vec::new();
            for trust in trusts {
                if seen_trusts.iter().any(|existing| existing == trust) {
                    return Err(CoreError::invalid_data("Duplicate trust entry in manifest"));
                }
                seen_trusts.push(trust.clone());
                if let ContractPermissionDescriptor::Group(pub_key) = trust {
                    if !pub_key.is_valid() {
                        return Err(CoreError::invalid_data(
                            "Invalid group public key in trusts",
                        ));
                    }
                }
            }
        }

        self.abi.validate()?;

        Ok(())
    }
}
