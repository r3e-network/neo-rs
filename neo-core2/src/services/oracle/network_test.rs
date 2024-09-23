use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use std::str::FromStr;

use crate::config;
use crate::oracle::{is_reserved, get_default_client, ErrRestrictedRedirect};
use reqwest::blocking::Client;
use reqwest::Error;
use std::error::Error as StdError;

#[test]
fn test_is_reserved() {
    assert!(is_reserved(Ipv4Addr::UNSPECIFIED));
    assert!(is_reserved(Ipv4Addr::new(10, 0, 0, 1)));
    assert!(is_reserved(Ipv4Addr::new(192, 168, 0, 1)));
    assert!(is_reserved(Ipv6Addr::from_str("ff01::1").unwrap())); // IPv6 interface-local all-nodes
    assert!(is_reserved(Ipv6Addr::LOCALHOST));

    assert!(!is_reserved(Ipv4Addr::new(8, 8, 8, 8)));
}

#[test]
fn test_default_client_restricted_redirect_err() {
    let cfg = config::OracleConfiguration {
        allow_private_host: false,
        request_timeout: Duration::from_secs(1),
    };
    let cl = get_default_client(cfg);

    let test_cases = vec![
        "http://localhost:8080",
        "http://localhost",
        "https://localhost:443",
        &format!("https://{}", Ipv4Addr::UNSPECIFIED),
        &format!("https://{}", Ipv4Addr::new(10, 0, 0, 1)),
        &format!("https://{}", Ipv4Addr::new(192, 168, 0, 1)),
        &format!("https://[{}]", Ipv6Addr::from_str("ff01::1").unwrap()), // IPv6 interface-local all-nodes
        &format!("https://[{}]", Ipv6Addr::LOCALHOST),
    ];

    for c in test_cases {
        let result = cl.get(c).send();
        match result {
            Ok(_) => panic!("Expected error, got success"),
            Err(err) => {
                assert!(err.is_redirect());
                assert!(err.to_string().contains("IP is not global unicast"), "{}", err);
            }
        }
    }
}
