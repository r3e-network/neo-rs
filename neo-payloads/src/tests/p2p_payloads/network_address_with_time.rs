use super::*;

#[test]
fn endpoint_uses_tcp_server_only_like_csharp() {
    let address = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
    let network_address = NetworkAddressWithTime::new(
        123,
        address,
        vec![
            NodeCapability::ws_server(30334),
            NodeCapability::tcp_server(10333),
        ],
    );

    assert_eq!(
        network_address.endpoint(),
        Some(SocketAddr::new(address, 10333))
    );
}

#[test]
fn endpoint_ignores_ws_server_like_csharp() {
    let address = IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10));
    let network_address =
        NetworkAddressWithTime::new(123, address, vec![NodeCapability::ws_server(30334)]);

    assert_eq!(network_address.endpoint(), None);
}
