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

/// Builds a POST request carrying the given `Content-Type`, or none at all when
/// `content_type` is `None`.
fn post_request(content_type: Option<&str>) -> jsonrpsee::server::HttpRequest<()> {
    let mut builder = jsonrpsee::server::HttpRequest::builder().method("POST");
    if let Some(value) = content_type {
        builder = builder.header("content-type", value);
    }
    builder.body(()).expect("request builds")
}

fn request_content_type(request: &jsonrpsee::server::HttpRequest<()>) -> &str {
    request
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
}

/// jsonrpsee answers `415 Unsupported Media Type` unless a request declares a
/// JSON content type, but the C# `RpcServer` plugin ignores the header entirely
/// and just parses the body. neo-go's RPC client depends on that leniency and
/// sends no `Content-Type` at all, so without this normalisation every neo-go
/// client fails against a neo-rs node on its very first call
/// (`failed to get network magic: HTTP 415/Unsupported Media Type`), which was
/// observed on a mixed private network.
#[test]
fn missing_content_type_is_normalized_to_json() {
    let mut request = post_request(None);
    normalize_json_content_type(&mut request);
    assert_eq!(
        request_content_type(&request),
        "application/json",
        "a body with no Content-Type must be treated as JSON, as C# does"
    );
}

#[test]
fn non_json_content_type_is_normalized_to_json() {
    for supplied in ["text/plain", "application/x-www-form-urlencoded", ""] {
        let mut request = post_request(Some(supplied));
        normalize_json_content_type(&mut request);
        assert_eq!(
            request_content_type(&request),
            "application/json",
            "Content-Type {supplied:?} must be normalized rather than rejected"
        );
    }
}

/// Content types jsonrpsee already accepts must survive untouched, parameters
/// and all, so nothing about the existing accepted path changes.
#[test]
fn json_content_types_are_left_untouched() {
    for supplied in [
        "application/json",
        "application/json; charset=utf-8",
        "application/json-rpc",
        "application/jsonrequest",
        "APPLICATION/JSON",
    ] {
        let mut request = post_request(Some(supplied));
        normalize_json_content_type(&mut request);
        assert_eq!(
            request_content_type(&request),
            supplied,
            "already-JSON Content-Type {supplied:?} must not be rewritten"
        );
    }
}

/// GET carries no body to interpret, so it must not gain a body content type.
#[test]
fn get_requests_are_left_untouched() {
    let mut request = jsonrpsee::server::HttpRequest::builder()
        .method("GET")
        .body(())
        .expect("request builds");
    normalize_json_content_type(&mut request);
    assert_eq!(
        request_content_type(&request),
        "",
        "GET has no body, so no Content-Type should be invented"
    );
}
