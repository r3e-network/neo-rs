use neo::prelude::*;
use neo::json::{Json, JsonValue};
use base64::{Engine as _, engine::general_purpose};
use std::convert::TryFrom;

#[derive(Clone, Debug)]
pub struct NEP6Contract {
    pub script: Vec<u8>,
    pub parameter_list: Vec<ContractParameterType>,
    pub parameter_names: Vec<String>,
    pub deployed: bool,
}

impl NEP6Contract {
    pub fn from_json(json: &Json) -> Option<Self> {
        if json.is_null() {
            return None;
        }

        let script = general_purpose::STANDARD.decode(json["script"].as_str()?).ok()?;
        let parameters = json["parameters"].as_array()?;
        let parameter_list = parameters
            .iter()
            .filter_map(|p| ContractParameterType::try_from(p["type"].as_u8()?).ok())
            .collect();
        let parameter_names = parameters
            .iter()
            .filter_map(|p| p["name"].as_str().map(String::from))
            .collect();
        let deployed = json["deployed"].as_bool()?;

        Some(Self {
            script,
            parameter_list,
            parameter_names,
            deployed,
        })
    }

    pub fn to_json(&self) -> Json {
        let mut contract = Json::new_object();
        contract["script"] = JsonValue::String(general_purpose::STANDARD.encode(&self.script));
        contract["parameters"] = JsonValue::Array(
            self.parameter_list
                .iter()
                .zip(&self.parameter_names)
                .map(|(type_, name)| {
                    let mut parameter = Json::new_object();
                    parameter["name"] = JsonValue::String(name.clone());
                    parameter["type"] = JsonValue::Number((*type_ as u8).into());
                    parameter
                })
                .collect(),
        );
        contract["deployed"] = JsonValue::Boolean(self.deployed);
        contract
    }
}
