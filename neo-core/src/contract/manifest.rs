// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};

use serde::{Deserialize, Serialize};

use crate::{
    contract::{NamedParamType, ParamType},
    types::{Extra, Sign},
    PublicKey,
};

pub const NEP11_NAME: &'static str = "NEP-11";
pub const NEP17_NAME: &'static str = "NEP-17";

pub const NEP11_PAYABLE: &'static str = "NEP-11-Payable";
pub const NEP17_PAYABLE: &'static str = "NEP-17-Payable";

pub const EMPTY_FEATURES: &'static str = "{}";


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    #[serde(rename = "pubKey")]
    pub public_key: PublicKey,

    #[serde(rename = "signature")]
    pub sign: Sign,
}

/// Empty at now.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Features {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permission {
    pub contract: String,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Method {
    pub name: String,
    pub parameters: Vec<NamedParamType>,
    pub offset: usize,
    pub return_type: ParamType,
    pub safe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub name: String,
    pub parameters: Vec<NamedParamType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Abi {
    pub methods: Vec<Method>,
    pub events: Vec<Event>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub groups: Vec<Group>,

    pub features: Features,

    #[serde(rename = "supportedstandards")]
    pub supported_standards: Vec<String>,

    pub abi: Abi,

    pub permissions: Vec<Permission>,

    pub trusts: Vec<String>,

    pub extra: Extra,
}
