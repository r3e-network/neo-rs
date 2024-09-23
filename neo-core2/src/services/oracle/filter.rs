use std::error::Error;
use std::str;
use serde_json::Value;
use crate::core::state;
use crate::services::oracle::jsonpath;

pub fn filter(value: &[u8], path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if !str::from_utf8(value).is_ok() {
        return Err("not an UTF-8".into());
    }

    let v: Value = serde_json::from_slice(value)?;
    let result = jsonpath::get(path, &v).ok_or("invalid filter")?;
    Ok(serde_json::to_vec(&result)?)
}

pub fn filter_request(result: &[u8], req: &state::OracleRequest) -> Result<Vec<u8>, Box<dyn Error>> {
    if let Some(filter) = &req.filter {
        return filter(result, filter);
    }
    Ok(result.to_vec())
}
