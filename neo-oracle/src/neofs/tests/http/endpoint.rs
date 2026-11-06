use super::super::super::http::normalize_neofs_endpoint;

#[test]
fn normalize_neofs_endpoint_requires_value() {
    assert!(normalize_neofs_endpoint("").is_err());
    assert!(normalize_neofs_endpoint("   ").is_err());
}

#[test]
fn normalize_neofs_endpoint_adds_scheme() {
    let normalized = normalize_neofs_endpoint("127.0.0.1:8080").expect("normalize endpoint");
    assert_eq!(normalized, "http://127.0.0.1:8080");
}

#[test]
fn normalize_neofs_endpoint_preserves_scheme() {
    let normalized = normalize_neofs_endpoint("https://neofs.example").expect("normalize endpoint");
    assert_eq!(normalized, "https://neofs.example");
}
