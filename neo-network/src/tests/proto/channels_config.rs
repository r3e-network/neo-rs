use super::*;

#[test]
fn default_config_has_expected_values() {
    let config = ChannelsConfig::default();
    assert!(config.enable_compression);
    assert_eq!(config.min_desired_connections, 10);
    assert_eq!(config.max_connections, 40);
}

#[test]
fn new_config_with_tcp() {
    let addr: SocketAddr = "127.0.0.1:10333".parse().unwrap();
    let config = ChannelsConfig::new(Some(addr));
    assert_eq!(config.tcp, Some(addr));
}
