use neo_serialization::json::{JObject, JToken};
use std::net::TcpListener;

pub(super) fn localhost_binding_permitted() -> bool {
    TcpListener::bind("127.0.0.1:0").is_ok()
}

pub(super) fn rpc_response(result: JToken) -> String {
    let mut response = JObject::new();
    response.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
    response.insert("id".to_string(), JToken::Number(1.0));
    response.insert("result".to_string(), result);
    JToken::Object(response).to_string()
}
