use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::response::RpcError;

#[derive(Clone, Debug)]
pub struct RpcParams(Option<Value>);

impl RpcParams {
    pub fn new(inner: Option<Value>) -> Self {
        Self(inner)
    }

    pub fn as_value(&self) -> Option<&Value> {
        self.0.as_ref()
    }

    pub fn parse<T: DeserializeOwned>(&self) -> Result<T, RpcError> {
        match &self.0 {
            Some(value) => serde_json::from_value(value.clone())
                .map_err(|err| RpcError::invalid_params(err.to_string())),
            None => serde_json::from_value(Value::Null)
                .map_err(|err| RpcError::invalid_params(err.to_string())),
        }
    }
}
