use std::net::{IpAddr, SocketAddr};

pub trait IpAddressExtensions {
    fn un_map(self) -> Self;
}

impl IpAddressExtensions for IpAddr {
    /// Checks if address is IPv4 Mapped to IPv6 format, if so, Map to IPv4.
    /// Otherwise, return current address.
    fn un_map(self) -> Self {
        match self {
            IpAddr::V6(v6) if v6.is_ipv4_mapped() => IpAddr::V4(v6.to_ipv4_mapped().unwrap()),
            _ => self,
        }
    }
}

impl IpAddressExtensions for SocketAddr {
    /// Checks if SocketAddr is IPv4 Mapped to IPv6 format, if so, unmap to IPv4.
    /// Otherwise, return current endpoint.
    fn un_map(self) -> Self {
        SocketAddr::new(self.ip().un_map(), self.port())
    }
}
