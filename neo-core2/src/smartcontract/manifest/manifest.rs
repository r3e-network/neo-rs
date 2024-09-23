use std::cmp::Ordering;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::error::Error;
use std::fmt;

use neo_core2::util::Uint160;
use neo_core2::vm::stackitem::{Item, Map, Null, Struct};
use ordered_json::OrderedJson;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_MANIFEST_SIZE: u16 = u16::MAX;

const NEP11_STANDARD_NAME: &str = "NEP-11";
const NEP17_STANDARD_NAME: &str = "NEP-17";
const NEP11_PAYABLE: &str = "NEP-11-Payable";
const NEP17_PAYABLE: &str = "NEP-17-Payable";

const EMPTY_FEATURES: &str = "{}";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub abi: ABI,
    pub features: Value,
    pub groups: Vec<Group>,
    pub permissions: Vec<Permission>,
    pub supported_standards: Vec<String>,
    pub trusts: WildPermissionDescs,
    pub extra: Value,
}

impl Manifest {
    pub fn new(name: String) -> Self {
        let mut m = Self {
            name,
            abi: ABI {
                methods: Vec::new(),
                events: Vec::new(),
            },
            features: serde_json::from_str(EMPTY_FEATURES).unwrap(),
            groups: Vec::new(),
            permissions: Vec::new(),
            supported_standards: Vec::new(),
            trusts: WildPermissionDescs::default(),
            extra: Value::Null,
        };
        m.trusts.restrict();
        m
    }

    pub fn default_manifest(name: String) -> Self {
        let mut m = Self::new(name);
        m.permissions = vec![Permission::new(PermissionKind::Wildcard)];
        m
    }

    pub fn can_call(&self, hash: &Uint160, to_call: &Manifest, method: &str) -> bool {
        self.permissions.iter().any(|p| p.is_allowed(hash, to_call, method))
    }

    pub fn is_valid(&self, hash: &Uint160, check_size: bool) -> Result<(), Box<dyn Error>> {
        if self.name.is_empty() {
            return Err("no name".into());
        }

        if self.supported_standards.contains(&String::new()) {
            return Err("invalid nameless supported standard".into());
        }
        if has_duplicates(&self.supported_standards) {
            return Err("duplicate supported standards".into());
        }
        self.abi.is_valid()?;

        if self.features.to_string().replace(&[' ', '\n', '\t', '\r'][..], "") != EMPTY_FEATURES {
            return Err("invalid features".into());
        }
        Groups::are_valid(&self.groups, hash)?;
        if self.trusts.value.is_none() && !self.trusts.wildcard {
            return Err("invalid (null?) trusts".into());
        }
        if let Some(trusts) = &self.trusts.value {
            if has_duplicates(trusts) {
                return Err("duplicate trusted contracts".into());
            }
        }
        Permissions::are_valid(&self.permissions)?;

        if check_size {
            let si = self.to_stack_item()?;
            // TODO: Implement stackitem::serialize
            // stackitem::serialize(&si)?;
        }

        Ok(())
    }

    pub fn is_standard_supported(&self, standard: &str) -> bool {
        self.supported_standards.contains(&standard.to_string())
    }

    pub fn to_stack_item(&self) -> Result<Item, Box<dyn Error>> {
        let groups: Vec<Item> = self.groups.iter().map(|g| g.to_stack_item()).collect();
        let supported_standards: Vec<Item> = self.supported_standards.iter().map(|s| Item::String(s.clone())).collect();
        let abi = self.abi.to_stack_item();
        let permissions: Vec<Item> = self.permissions.iter().map(|p| p.to_stack_item()).collect();
        let trusts = if self.trusts.is_wildcard() {
            Item::Null
        } else {
            Item::Array(self.trusts.value.as_ref().unwrap().iter().map(|v| v.to_stack_item()).collect())
        };
        let extra = extra_to_stack_item(&self.extra);

        Ok(Item::Struct(Struct::new(vec![
            Item::String(self.name.clone()),
            Item::Array(groups),
            Item::Map(Map::new()),
            Item::Array(supported_standards),
            abi,
            Item::Array(permissions),
            trusts,
            extra,
        ])))
    }

    pub fn from_stack_item(item: &Item) -> Result<Self, Box<dyn Error>> {
        if let Item::Struct(s) = item {
            if s.value().len() != 8 {
                return Err("invalid stackitem length".into());
            }

            let name = s.value()[0].try_string()?;
            let groups = s.value()[1].try_array()?;
            let features = s.value()[2].try_map()?;
            let supported_standards = s.value()[3].try_array()?;
            let abi = ABI::from_stack_item(&s.value()[4])?;
            let permissions = s.value()[5].try_array()?;
            let trusts = s.value()[6].clone();
            let extra = s.value()[7].try_bytes()?;

            if !features.is_empty() {
                return Err("invalid Features stackitem".into());
            }

            Ok(Self {
                name,
                abi,
                features: serde_json::from_str(EMPTY_FEATURES).unwrap(),
                groups: groups.iter().map(Group::from_stack_item).collect::<Result<_, _>>()?,
                permissions: permissions.iter().map(Permission::from_stack_item).collect::<Result<_, _>>()?,
                supported_standards: supported_standards.iter().map(|i| i.try_string()).collect::<Result<_, _>>()?,
                trusts: match trusts {
                    Item::Null => WildPermissionDescs::default(),
                    Item::Array(arr) => WildPermissionDescs {
                        value: Some(arr.iter().map(PermissionDesc::from_stack_item).collect::<Result<_, _>>()?),
                        wildcard: false,
                    },
                    _ => return Err("invalid Trusts stackitem type".into()),
                },
                extra: serde_json::from_slice(&extra)?,
            })
        } else {
            Err("invalid Manifest stackitem type".into())
        }
    }
}

fn extra_to_stack_item(raw_extra: &Value) -> Item {
    if raw_extra.is_null() {
        return Item::String("null".to_string());
    }

    let ordered: OrderedJson = serde_json::from_value(raw_extra.clone()).unwrap();
    let res = serde_json::to_vec(&ordered).unwrap();
    Item::ByteArray(res)
}

fn has_duplicates<T: Ord>(slice: &[T]) -> bool {
    let mut sorted = slice.to_vec();
    sorted.sort();
    sorted.windows(2).any(|w| w[0] == w[1])
}

// Note: Implement other structs (ABI, Group, Permission, PermissionDesc, WildPermissionDescs) similarly
