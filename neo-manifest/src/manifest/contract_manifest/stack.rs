//! VM stack-value projection for `ContractManifest`.

use neo_error::CoreError;
use neo_vm::{Interoperable, InteroperableError};
use neo_vm_rs::StackValue;
use serde_json::Value;

use crate::manifest::{
    ContractAbi, ContractGroup, ContractManifest, ContractPermission, ContractPermissionDescriptor,
    WildCardContainer,
};

impl Interoperable for ContractManifest {
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError> {
        ContractManifest::from_stack_value(self, value)
            .map_err(|e| InteroperableError::InvalidData(e.to_string()))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(ContractManifest::to_stack_value(self))
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

impl ContractManifest {
    /// Converts the manifest to the VM stack-value shape used by native interop.
    pub fn to_stack_value(&self) -> StackValue {
        let group_items = self
            .groups
            .iter()
            .map(ContractGroup::to_stack_value)
            .collect::<Vec<_>>();

        let standards_items = self
            .supported_standards
            .iter()
            .map(|standard| StackValue::ByteString(standard.as_bytes().to_vec()))
            .collect::<Vec<_>>();

        let permission_items = self
            .permissions
            .iter()
            .map(ContractPermission::to_stack_value)
            .collect::<Vec<_>>();

        let trusts_item = match &self.trusts {
            WildCardContainer::Wildcard => StackValue::Null,
            WildCardContainer::List(trusts) => StackValue::Array(
                trusts
                    .iter()
                    .map(ContractPermissionDescriptor::to_stack_value)
                    .collect(),
            ),
        };

        let extra_bytes = match &self.extra {
            Some(extra) => neo_serialization::JsonSerializer::encode_value_csharp_compatible(extra),
            None => b"null".to_vec(),
        };

        StackValue::Struct(vec![
            StackValue::ByteString(self.name.as_bytes().to_vec()),
            StackValue::Array(group_items),
            // C# ContractManifest.ToStackItem always emits an empty features map.
            StackValue::Map(Vec::new()),
            StackValue::Array(standards_items),
            self.abi.to_stack_value(),
            StackValue::Array(permission_items),
            trusts_item,
            StackValue::ByteString(extra_bytes),
        ])
    }

    /// Populates the manifest from the VM stack-value shape used by native interop.
    pub fn from_stack_value(
        &mut self,
        stack_value: StackValue,
    ) -> std::result::Result<(), CoreError> {
        let StackValue::Struct(items) = stack_value else {
            return Err(CoreError::invalid_format(
                "ContractManifest expects Struct stack value",
            ));
        };

        if items.len() < 8 {
            return Err(CoreError::invalid_format(format!(
                "ContractManifest stack value must contain 8 elements, found {}",
                items.len()
            )));
        }

        let name_bytes = items[0]
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_format("ContractManifest name must be ByteString"))?;
        self.name = String::from_utf8(name_bytes)
            .map_err(|_| CoreError::invalid_format("ContractManifest name must be valid UTF-8"))?;

        self.groups = match &items[1] {
            StackValue::Array(groups) => {
                let mut values = Vec::with_capacity(groups.len());
                for item in groups {
                    values.push(ContractGroup::try_from_stack_value(item.clone())?);
                }
                values
            }
            _ => {
                return Err(CoreError::invalid_format(
                    "ContractManifest groups must be an Array",
                ));
            }
        };

        if let StackValue::Map(features) = &items[2] {
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
            StackValue::Array(standards) => {
                let mut values = Vec::with_capacity(standards.len());
                for item in standards {
                    if matches!(item, StackValue::Null) {
                        return Err(CoreError::invalid_format(
                            "ContractManifest supported standard must not be null",
                        ));
                    }
                    let bytes = item.to_byte_string_bytes().ok_or_else(|| {
                        CoreError::invalid_format(
                            "ContractManifest supported standard must be ByteString",
                        )
                    })?;
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
        abi.from_stack_value(items[4].clone())?;
        self.abi = abi;

        self.permissions = match &items[5] {
            StackValue::Array(permissions) => {
                let mut values = Vec::new();
                for item in permissions {
                    let mut permission = ContractPermission::default_wildcard();
                    permission.from_stack_value(item.clone())?;
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
            StackValue::Null => WildCardContainer::create_wildcard(),
            StackValue::Array(trusts) => {
                let mut values = Vec::with_capacity(trusts.len());
                for item in trusts {
                    values.push(ContractPermissionDescriptor::from_stack_value(
                        item.clone(),
                    )?);
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
            StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => {
                parse_extra_bytes(bytes.as_slice())?
            }
            other => {
                return Err(CoreError::invalid_format(format!(
                    "ContractManifest extra must be ByteString, found {:?}",
                    other.compact_type_tag()
                )));
            }
        };

        Ok(())
    }
}

fn parse_extra_bytes(bytes: &[u8]) -> Result<Option<Value>, CoreError> {
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

    match serde_json::from_str::<Value>(text) {
        Ok(Value::Null) => Ok(None),
        Ok(value @ Value::Object(_)) => Ok(Some(value)),
        Ok(_) => Err(CoreError::invalid_format(
            "ContractManifest extra must be a JSON object or null",
        )),
        Err(e) => Err(CoreError::invalid_format(format!(
            "Invalid JSON in manifest extra: {e}"
        ))),
    }
}
