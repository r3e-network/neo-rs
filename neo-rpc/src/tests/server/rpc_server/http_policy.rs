use super::*;

fn header<'a>(response: &'a jsonrpsee::server::HttpResponse, name: &str) -> &'a str {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

#[test]
fn unauthorized_response_has_basic_auth_policy() {
    let response = unauthorized_response();

    assert_eq!(response.status().as_u16(), 401);
    assert_eq!(
        header(&response, "content-type"),
        "text/plain; charset=utf-8"
    );
    assert_eq!(
        header(&response, "www-authenticate"),
        "Basic realm=\"neo-rpc\""
    );
}

#[test]
fn allowed_preflight_response_has_cors_policy() {
    let response = preflight_response(Some(RpcCorsHeaders {
        allow_origin: "https://wallet.example".to_string(),
        allow_headers: "content-type, authorization".to_string(),
    }));

    assert_eq!(response.status().as_u16(), 204);
    assert_eq!(
        header(&response, "access-control-allow-origin"),
        "https://wallet.example"
    );
    assert_eq!(
        header(&response, "access-control-allow-methods"),
        "POST, OPTIONS"
    );
    assert_eq!(
        header(&response, "access-control-allow-headers"),
        "content-type, authorization"
    );
    assert_eq!(header(&response, "access-control-max-age"), "600");
    assert_eq!(header(&response, "vary"), "Origin");
}

#[test]
fn rejected_preflight_response_is_plain_forbidden() {
    let response = preflight_response(None);

    assert_eq!(response.status().as_u16(), 403);
    assert_eq!(
        header(&response, "content-type"),
        "text/plain; charset=utf-8"
    );
    assert_eq!(header(&response, "access-control-allow-origin"), "");
}
