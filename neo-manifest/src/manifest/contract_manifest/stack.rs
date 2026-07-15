//! VM stack-item projection for `ContractManifest`.

use neo_error::CoreError;
use neo_vm::{Interoperable, InteroperableError, StackItem, VmOrderedDictionary};

use crate::manifest::{
    ContractAbi, ContractGroup, ContractManifest, ContractPermission, ContractPermissionDescriptor,
    ManifestExtra, WildCardContainer,
};

impl Interoperable for ContractManifest {
    fn from_stack_item(&mut self, value: StackItem) -> Result<(), InteroperableError> {
        ContractManifest::from_stack_item(self, value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(ContractManifest::to_stack_item(self))
    }
}

impl ContractManifest {
    /// Converts the manifest to the VM stack-item shape used by native interop.
    pub fn to_stack_item(&self) -> StackItem {
        let group_items = self
            .groups
            .iter()
            .map(ContractGroup::to_stack_item)
            .collect::<Vec<_>>();

        let standards_items = self
            .supported_standards
            .iter()
            .map(|standard| StackItem::from_byte_string(standard.as_bytes().to_vec()))
            .collect::<Vec<_>>();

        let permission_items = self
            .permissions
            .iter()
            .map(ContractPermission::to_stack_item)
            .collect::<Vec<_>>();

        let trusts_item = match &self.trusts {
            WildCardContainer::Wildcard => StackItem::Null,
            WildCardContainer::List(trusts) => StackItem::from_array(
                trusts
                    .iter()
                    .map(ContractPermissionDescriptor::to_stack_item)
                    .collect(),
            ),
        };

        let extra_bytes = match &self.extra {
            Some(extra) => {
                neo_serialization::JsonSerializer::encode_value_csharp_compatible(extra.as_value())
            }
            None => b"null".to_vec(),
        };

        StackItem::from_struct(vec![
            StackItem::from_byte_string(self.name.as_bytes().to_vec()),
            StackItem::from_array(group_items),
            // C# ContractManifest.ToStackItem always emits an empty features map.
            StackItem::from_map(VmOrderedDictionary::new()),
            StackItem::from_array(standards_items),
            self.abi.to_stack_item(),
            StackItem::from_array(permission_items),
            trusts_item,
            StackItem::from_byte_string(extra_bytes),
        ])
    }

    /// Populates the manifest from the VM stack-item shape used by native interop.
    pub fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), CoreError> {
        let StackItem::Struct(structure) = stack_item else {
            return Err(CoreError::invalid_format(
                "ContractManifest expects Struct stack item",
            ));
        };
        let items = structure.items();

        if items.len() < 8 {
            return Err(CoreError::invalid_format(format!(
                "ContractManifest stack item must contain 8 elements, found {}",
                items.len()
            )));
        }

        let name_bytes = byte_like_data(&items[0], "ContractManifest name")?;
        self.name = String::from_utf8(name_bytes)
            .map_err(|_| CoreError::invalid_format("ContractManifest name must be valid UTF-8"))?;

        self.groups = match &items[1] {
            StackItem::Array(groups) => {
                let groups = groups.items();
                let mut values = Vec::with_capacity(groups.len());
                for item in &groups {
                    values.push(ContractGroup::try_from_stack_item(item)?);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest groups must be an Array",
                ));
            }
        };

        if let StackItem::Map(features) = &items[2] {
            if !features.is_empty() {
                return Err(CoreError::invalid_format(
                    "ContractManifest features map must be empty",
                ));
            }
        } else {
            return Err(CoreError::invalid_format(
                "ContractManifest features must be a Map",
            ));
        }
        self.features.clear();

        self.supported_standards = match &items[3] {
            StackItem::Array(standards) => {
                let standards = standards.items();
                let mut values = Vec::with_capacity(standards.len());
                for item in &standards {
                    if item.is_null() {
                        return Err(CoreError::invalid_format(
                            "ContractManifest supported standard must not be null",
                        ));
                    }
                    let bytes = byte_like_data(item, "ContractManifest supported standard")?;
                    let standard = String::from_utf8(bytes).map_err(|_| {
                        CoreError::invalid_format(
                            "ContractManifest supported standard must be valid UTF-8",
                        )
                    })?;
                    values.push(standard);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest supported standards must be an Array",
                ));
            }
        };

        let mut abi = ContractAbi::default();
        abi.from_stack_item(items[4].clone())?;
        self.abi = abi;

        self.permissions = match &items[5] {
            StackItem::Array(permissions) => {
                let mut values = Vec::new();
                for item in permissions.items() {
                    let mut permission = ContractPermission::default_wildcard();
                    permission.from_stack_item(item)?;
                    values.push(permission);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest permissions must be an Array",
                ));
            }
        };

        self.trusts = match &items[6] {
            StackItem::Null => WildCardContainer::create_wildcard(),
            StackItem::Array(trusts) => {
                let trusts = trusts.items();
                let mut values = Vec::with_capacity(trusts.len());
                for item in &trusts {
                    values.push(ContractPermissionDescriptor::from_stack_item(item)?);
                }
                WildCardContainer::create(values)
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest trusts must be Null or Array",
                ));
            }
        };

        self.extra = match &items[7] {
            StackItem::ByteString(bytes) => parse_extra_bytes(bytes)?,
            StackItem::Buffer(buffer) => parse_extra_bytes(&buffer.data())?,
            other => {
                return Err(CoreError::invalid_format(format!(
                    "ContractManifest extra must be ByteString, found {:?}",
                    other.stack_item_type()
                )));
            }
        };

        Ok(())
    }
}

fn byte_like_data(item: &StackItem, field: &str) -> Result<Vec<u8>, CoreError> {
    match item {
        StackItem::ByteString(bytes) => Ok(bytes.clone()),
        StackItem::Buffer(buffer) => Ok(buffer.data()),
        _ => Err(CoreError::invalid_format(format!(
            "{field} must be ByteString"
        ))),
    }
}

fn parse_extra_bytes(bytes: &[u8]) -> Result<Option<ManifestExtra>, CoreError> {
    if bytes.is_empty() {
        return Err(CoreError::invalid_format(
            "ContractManifest extra must not be empty",
        ));
    }

    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            return Err(CoreError::invalid_format(
                "ContractManifest extra must be valid UTF-8",
            ));
        }
    };

    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(serde_json::Value::Null) => Ok(None),
        Ok(value @ serde_json::Value::Object(_)) => Ok(Some(ManifestExtra::from_value(value)?)),
        Ok(_) => Err(CoreError::invalid_format(
            "ContractManifest extra must be a JSON object or null",
        )),
        Err(e) => Err(CoreError::invalid_format(format!(
            "Invalid JSON in manifest extra: {e}"
        ))),
    }
}
