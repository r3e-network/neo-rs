// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum Version {
    #[serde(rename = "2.0")]
    V2_0,
}

/// Request struct for jsonrpc 2.0. Params should be a tuple
#[derive(Debug, Serialize, Deserialize)]
pub struct Request<Params> {
    #[serde(rename = "jsonrpc")]
    pub version: Version,

    pub id: u64,
    pub method: String,
    pub params: Params,
}

/// Response struct for jsonrpc 2.0.
#[derive(Debug, Serialize, Deserialize)]
pub struct Response<Result> {
    #[serde(rename = "jsonrpc")]
    pub version: Version,

    pub id: u64,
    pub result: Result,
}

#[cfg(test)]
mod test {
    use super::*;

    #[derive(Debug, Serialize, Deserialize)]
    pub struct Foo(String, u64, #[serde(skip_serializing_if = "String::is_empty")] String);

    #[test]
    fn test_serde_tuple() {
        let foo = Foo("hello".into(), 1, "".into());
        let json = serde_json::to_string(&foo).expect("`to_string` should be ok");
        assert_eq!(&json, r#"["hello",1]"#); // skipped as expected
    }
}
