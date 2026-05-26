use neo_json::{JArray, JObject, JToken};
use std::fs;
use std::path::PathBuf;

pub(crate) fn rpc_case_request(name: &str) -> Option<JObject> {
    let case = rpc_case(name)?;
    Some(
        case.get("Request")
            .and_then(JToken::as_object)
            .expect("case request")
            .clone(),
    )
}

pub(crate) fn rpc_case_result(name: &str) -> Option<JObject> {
    let response = rpc_case_response(name)?;
    Some(
        response
            .get("result")
            .and_then(JToken::as_object)
            .expect("case result")
            .clone(),
    )
}

pub(crate) fn rpc_case_response(name: &str) -> Option<JObject> {
    let case = rpc_case(name)?;
    Some(
        case.get("Response")
            .and_then(JToken::as_object)
            .expect("case response")
            .clone(),
    )
}

pub(crate) fn rpc_case_result_array(name: &str) -> Option<JArray> {
    let response = rpc_case_response(name)?;
    Some(
        response
            .get("result")
            .and_then(JToken::as_array)
            .expect("case result")
            .clone(),
    )
}

pub(crate) fn rpc_case_params(name: &str) -> Option<JArray> {
    let request = rpc_case_request(name)?;
    Some(
        request
            .get("params")
            .and_then(JToken::as_array)
            .expect("case params")
            .clone(),
    )
}

fn rpc_case(name: &str) -> Option<JObject> {
    let path = rpc_test_cases_path();
    if !path.exists() {
        eprintln!(
            "SKIP: neo_csharp submodule not initialized ({})",
            path.display()
        );
        return None;
    }

    let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
    let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
    let cases = token
        .as_array()
        .expect("RpcTestCases.json should be an array");

    for entry in cases.children() {
        let token = entry.as_ref().expect("array entry");
        let obj = token.as_object().expect("case object");
        let case_name = obj
            .get("Name")
            .and_then(JToken::as_string)
            .unwrap_or_default();
        if case_name.eq_ignore_ascii_case(name) {
            return Some(obj.clone());
        }
    }

    eprintln!("SKIP: RpcTestCases.json missing case: {name}");
    None
}

fn rpc_test_cases_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("neo_csharp");
    path.push("node");
    path.push("tests");
    path.push("Neo.Network.RPC.Tests");
    path.push("RpcTestCases.json");
    path
}
