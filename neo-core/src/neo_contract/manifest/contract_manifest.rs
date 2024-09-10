
use neo::prelude::*;
use neo::io::*;
use neo::vm::*;
use neo::types::*;
use neo::json::Json;
use std::collections::HashMap;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::stack_item::StackItem;
use crate::neo_contract::iinteroperable::IInteroperable;
use crate::neo_contract::manifest::contract_abi::ContractAbi;
use crate::neo_contract::manifest::contract_group::ContractGroup;
use crate::neo_contract::manifest::contract_permission::ContractPermission;
use crate::neo_contract::manifest::contract_permission_descriptor::ContractPermissionDescriptor;
use crate::neo_contract::manifest::wild_card_container::WildcardContainer;
use crate::uint160::UInt160;

/// Represents the manifest of a smart contract.
/// When a smart contract is deployed, it must explicitly declare the features and permissions it will use.
/// When it is running, it will be limited by its declared list of features and permissions, and cannot make any behavior beyond the scope of the list.
///
/// For more details, see NEP-15.
#[derive(Clone, Debug)]
pub struct ContractManifest {
    /// The name of the contract.
    pub name: String,

    /// The groups of the contract.
    pub groups: Vec<ContractGroup>,

    /// Indicates which standards the contract supports. It can be a list of NEPs.
    pub supported_standards: Vec<String>,

    /// The ABI of the contract.
    pub abi: ContractAbi,

    /// The permissions of the contract.
    pub permissions: Vec<ContractPermission>,

    /// The trusted contracts and groups of the contract.
    /// If a contract is trusted, the user interface will not give any warnings when called by the contract.
    pub trusts: WildcardContainer<ContractPermissionDescriptor>,

    /// Custom user data.
    pub extra: Option<Json>,
}

impl ContractManifest {
    /// The maximum length of a manifest.
    pub const MAX_LENGTH: usize = u16::MAX as usize;

    /// Converts the manifest from a JSON object.
    ///
    /// # Arguments
    ///
    /// * `json` - The manifest represented by a JSON object.
    ///
    /// # Returns
    ///
    /// The converted manifest.
    pub fn from_json(json: &Json) -> Result<Self, Error> {
        let name = json["name"].as_str().ok_or(Error::Format)?.to_string();
        if name.is_empty() {
            return Err(Error::Format);
        }

        let groups = json["groups"]
            .as_array()
            .map(|arr| arr.iter().map(|u| ContractGroup::from_json(u)).collect::<Result<Vec<_>, _>>())
            .unwrap_or_else(|| Ok(vec![]))?;

        let supported_standards = json["supportedstandards"]
            .as_array()
            .map(|arr| arr.iter().map(|u| u.as_str().ok_or(Error::Format).map(String::from)).collect::<Result<Vec<_>, _>>())
            .unwrap_or_else(|| Ok(vec![]))?;

        if supported_standards.iter().any(|s| s.is_empty()) {
            return Err(Error::Format);
        }

        let abi = ContractAbi::from_json(&json["abi"])?;
        let permissions = json["permissions"]
            .as_array()
            .map(|arr| arr.iter().map(|u| ContractPermission::from_json(u)).collect::<Result<Vec<_>, _>>())
            .unwrap_or_else(|| Ok(vec![]))?;

        let trusts = WildcardContainer::from_json(&json["trusts"], |u| ContractPermissionDescriptor::from_json(u))?;
        let extra = json["extra"].as_object().cloned();

        if json["features"].as_object().map_or(false, |obj| !obj.is_empty()) {
            return Err(Error::Format);
        }

        // Validate uniqueness
        let _ = groups.iter().map(|g| &g.pub_key).collect::<HashMap<_, _>>();
        let _ = supported_standards.iter().collect::<HashMap<_, _>>();
        let _ = permissions.iter().map(|p| &p.contract).collect::<HashMap<_, _>>();
        let _ = trusts.iter().collect::<HashMap<_, _>>();

        Ok(Self {
            name,
            groups,
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        })
    }

    /// Parse the manifest from a byte array containing JSON data.
    ///
    /// # Arguments
    ///
    /// * `json` - The byte array containing JSON data.
    ///
    /// # Returns
    ///
    /// The parsed manifest.
    pub fn parse(json: &[u8]) -> Result<Self, Error> {
        if json.len() > Self::MAX_LENGTH {
            return Err(Error::InvalidArgument);
        }
        let json = Json::from_slice(json)?;
        Self::from_json(&json)
    }

    /// Converts the manifest to a JSON object.
    ///
    /// # Returns
    ///
    /// The manifest represented by a JSON object.
    pub fn to_json(&self) -> Json {
        let mut json = Json::new_object();
        json.insert("name", Json::from(self.name.clone()));
        json.insert("groups", Json::from(self.groups.iter().map(|u| u.to_json()).collect::<Vec<_>>()));
        json.insert("features", Json::new_object());
        json.insert("supportedstandards", Json::from(self.supported_standards.iter().cloned().collect::<Vec<_>>()));
        json.insert("abi", self.abi.to_json());
        json.insert("permissions", Json::from(self.permissions.iter().map(|p| p.to_json()).collect::<Vec<_>>()));
        json.insert("trusts", self.trusts.to_json(|p| p.to_json()));
        if let Some(extra) = &self.extra {
            json.insert("extra", extra.clone());
        }
        json
    }

    /// Determines whether the manifest is valid.
    ///
    /// # Arguments
    ///
    /// * `limits` - The `ExecutionEngineLimits` used for test serialization.
    /// * `hash` - The hash of the contract.
    ///
    /// # Returns
    ///
    /// `true` if the manifest is valid; otherwise, `false`.
    pub fn is_valid(&self, limits: &ExecutionEngineLimits, hash: &UInt160) -> bool {
        // Ensure that is serializable
        if let Err(_) = self.to_stack_item().serialize(limits) {
            return false;
        }
        // Check groups
        self.groups.iter().all(|u| u.is_valid(hash))
    }
}

impl IInteroperable for ContractManifest {
    fn from_stack_item(stack_item: &StackItem) -> Result<Self, Error> {
        let s = stack_item.as_struct()?;
        if s.len() != 8 {
            return Err(Error::InvalidStructure);
        }
        let name = s[0].as_string()?;
        let groups = s[1].as_array()?.iter().map(|p| ContractGroup::from_stack_item(p)).collect::<Result<Vec<_>, _>>()?;
        if !s[2].as_map()?.is_empty() {
            return Err(Error::InvalidArgument);
        }
        let supported_standards = s[3].as_array()?.iter().map(|p| p.as_string()).collect::<Result<Vec<_>, _>>()?;
        let abi = ContractAbi::from_stack_item(&s[4])?;
        let permissions = s[5].as_array()?.iter().map(|p| ContractPermission::from_stack_item(p)).collect::<Result<Vec<_>, _>>()?;
        let trusts = match &s[6] {
            StackItem::Null => WildcardContainer::create_wildcard(),
            StackItem::Array(array) => WildcardContainer::create(array.iter().map(ContractPermissionDescriptor::from_stack_item).collect::<Result<Vec<_>, _>>()?),
            _ => return Err(Error::InvalidArgument),
        };
        let extra = Json::parse(&s[7].as_string()?).ok();

        Ok(Self {
            name,
            groups,
            supported_standards,
            abi,
            permissions,
            trusts,
            extra,
        })
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::Struct(vec![
            StackItem::String(self.name.clone()),
            StackItem::Array(self.groups.iter().map(|p| p.to_stack_item()).collect()),
            StackItem::Map(HashMap::new()),
            StackItem::Array(self.supported_standards.iter().map(|p| StackItem::String(p.clone())).collect()),
            self.abi.to_stack_item(),
            StackItem::Array(self.permissions.iter().map(|p| p.to_stack_item()).collect()),
            if self.trusts.is_wildcard() {
                StackItem::Null
            } else {
                StackItem::Array(self.trusts.iter().map(|p| p.to_stack_item()).collect())
            },
            StackItem::String(self.extra.as_ref().map_or("null".to_string(), |e| e.to_string())),
        ])
    }
}
