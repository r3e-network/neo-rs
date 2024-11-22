use crate::json_error::JsonError;

pub trait JsonConvertibleTrait {
    fn to_json(&self) -> serde_json::Value;
    fn from_json(json: &serde_json::Value) -> Result<Self, JsonError>
    where Self: Sized;
}
