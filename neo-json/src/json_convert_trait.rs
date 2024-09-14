use crate::json_error::JsonError;
use crate::jtoken::JToken;

pub trait IJsonConvertible {
    fn to_json(&self) -> JToken;
    fn from_json(json: &JToken) -> Result<Self, JsonError>
    where
        Self: Sized;
}
