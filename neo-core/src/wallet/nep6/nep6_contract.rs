use std::convert::TryFrom;
use neo_json::jtoken::JToken;
use crate::neo_contract::contract_parameter_type::ContractParameterType;

#[derive(Clone, Debug)]
pub struct NEP6Contract {
    pub script: Vec<u8>,
    pub parameter_list: Vec<ContractParameterType>,
    pub parameter_names: Vec<String>,
    pub deployed: bool,
}

impl NEP6Contract {
    pub fn from_json(json: &JToken) -> Option<Self> {
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

    pub fn to_json(&self) -> JToken {
        let mut contract = JToken::new_object();
        contract["script"] = JToken::String(general_purpose::STANDARD.encode(&self.script));
        contract["parameters"] = JToken::Array(
            self.parameter_list
                .iter()
                .zip(&self.parameter_names)
                .map(|(type_, name)| {
                    let mut parameter = JToken::new_object();
                    parameter["name"] = JToken::String(name.clone());
                    parameter["type"] = JToken::Number((*type_ as u8).into());
                    parameter
                })
                .collect(),
        );
        contract["deployed"] = JToken::Boolean(self.deployed);
        contract
    }
}
