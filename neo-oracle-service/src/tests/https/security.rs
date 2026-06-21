use super::*;

#[test]
fn test_is_internal_ip_v4() {
    assert!(Ssrf::is_internal_ip("127.0.0.1".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("10.0.0.1".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("192.168.1.1".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("172.16.0.1".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("0.0.0.0".parse().unwrap()));
    assert!(!Ssrf::is_internal_ip("8.8.8.8".parse().unwrap()));
    assert!(!Ssrf::is_internal_ip("1.1.1.1".parse().unwrap()));
}

#[test]
fn test_is_internal_ip_v6() {
    assert!(Ssrf::is_internal_ip("::1".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("::".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("fc00::1".parse().unwrap()));
    assert!(Ssrf::is_internal_ip("fe80::1".parse().unwrap()));
    assert!(!Ssrf::is_internal_ip(
        "2001:4860:4860::8888".parse().unwrap()
    ));
}

#[test]
fn test_is_localhost_name() {
    assert!(Ssrf::is_localhost_name("localhost"));
    assert!(Ssrf::is_localhost_name("LOCALHOST"));
    assert!(Ssrf::is_localhost_name("localhost.localdomain"));
    assert!(Ssrf::is_localhost_name("myhost.local"));
    assert!(!Ssrf::is_localhost_name("example.com"));
}

#[test]
fn test_validate_url_for_ssrf() {
    assert!(Ssrf::validate_url_for_ssrf("https://example.com").is_ok());
    assert!(Ssrf::validate_url_for_ssrf("http://example.com/path").is_ok());
    assert!(Ssrf::validate_url_for_ssrf("ftp://example.com").is_err());
    assert!(Ssrf::validate_url_for_ssrf("https://user:pass@example.com").is_err());
}
