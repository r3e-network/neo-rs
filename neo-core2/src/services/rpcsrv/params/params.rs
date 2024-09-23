use serde::{Serialize, Deserialize};
use serde_json::{self, Value as JsonValue};
use std::fmt;

#[derive(Serialize, Deserialize, Debug)]
pub struct Param {
    #[serde(flatten)]
    raw_message: JsonValue,
}

// Params represents the JSON-RPC params.
pub type Params = Vec<Param>;

// FromAny allows to create Params for a slice of abstract values (by
// JSON-marshaling them).
pub fn from_any(arr: Vec<impl Serialize>) -> Result<Params, String> {
    let mut res = Params::new();
    for (i, item) in arr.into_iter().enumerate() {
        let b = serde_json::to_value(item).map_err(|e| format!("wrong parameter {}: {}", i, e))?;
        res.push(Param { raw_message: b });
    }
    Ok(res)
}

// Value returns the param struct for the given
// index if it exists.
impl Params {
    pub fn value(&self, index: usize) -> Option<&Param> {
        self.get(index)
    }
}

impl fmt::Display for Params {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
