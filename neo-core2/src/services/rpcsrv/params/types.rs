use serde::{Deserialize, Serialize};
use serde_json::{self, Value as JsonValue};
use std::error::Error;
use std::fmt;
use std::io::{self, Read};

const MAX_BATCH_SIZE: usize = 100;

#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    #[serde(flatten)]
    pub in_request: Option<In>,
    #[serde(flatten)]
    pub batch: Option<Batch>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct In {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Vec<Param>,
    #[serde(default)]
    pub id: Option<JsonValue>,
}

pub type Batch = Vec<In>;

impl Request {
    pub fn new() -> Self {
        Request {
            in_request: None,
            batch: None,
        }
    }

    pub fn decode_data<R: Read>(&mut self, data: R) -> Result<(), Box<dyn Error>> {
        let raw_data: JsonValue = serde_json::from_reader(data)?;
        self.unmarshal_json(&raw_data)
    }

    pub fn unmarshal_json(&mut self, data: &JsonValue) -> Result<(), Box<dyn Error>> {
        if let Ok(in_request) = serde_json::from_value::<In>(data.clone()) {
            self.in_request = Some(in_request);
            return Ok(());
        }

        if let Some(array) = data.as_array() {
            if array.len() > MAX_BATCH_SIZE {
                return Err(format!("the number of requests in batch shouldn't exceed {}", MAX_BATCH_SIZE).into());
            }

            let mut batch = Batch::new();
            for item in array {
                let in_request: In = serde_json::from_value(item.clone())?;
                batch.push(in_request);
            }

            if batch.is_empty() {
                return Err("empty request".into());
            }

            self.batch = Some(batch);
            return Ok(());
        }

        Err("invalid JSON-RPC request".into())
    }
}

impl In {
    pub fn new() -> Self {
        In {
            jsonrpc: "2.0".to_string(),
            method: String::new(),
            params: Vec::new(),
            id: None,
        }
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(ref in_request) = self.in_request {
            write!(f, "{:?}", in_request)
        } else if let Some(ref batch) = self.batch {
            write!(f, "{:?}", batch)
        } else {
            write!(f, "Empty Request")
        }
    }
}
