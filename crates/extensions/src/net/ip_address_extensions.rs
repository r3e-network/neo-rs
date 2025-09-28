// Copyright (C) 2015-2025 The Neo Project.
//
// ip_address_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::net::{IpAddr, SocketAddr};

/// IP address extensions matching C# IpAddressExtensions exactly
pub trait IpAddressExtensions {
    /// Checks if address is IPv4 Mapped to IPv6 format, if so, Map to IPv4.
    /// Otherwise, return current address.
    /// Matches C# UnMap method
    fn un_map(&self) -> IpAddr;
}

/// IP endpoint extensions matching C# IpAddressExtensions exactly
pub trait IpEndPointExtensions {
    /// Checks if IPEndPoint is IPv4 Mapped to IPv6 format, if so, unmap to IPv4.
    /// Otherwise, return current endpoint.
    /// Matches C# UnMap method
    fn un_map(&self) -> SocketAddr;
}

impl IpAddressExtensions for IpAddr {
    fn un_map(&self) -> IpAddr {
        match self {
            IpAddr::V6(ipv6) => {
                // Check if it's an IPv4-mapped IPv6 address
                if ipv6.segments() == [0, 0, 0, 0, 0, 0xffff, 0, 0] {
                    // Extract IPv4 address from the last 32 bits
                    let bytes = ipv6.octets();
                    let ipv4_bytes = [bytes[12], bytes[13], bytes[14], bytes[15]];
                    IpAddr::V4(std::net::Ipv4Addr::from(ipv4_bytes))
                } else {
                    *self
                }
            }
            IpAddr::V4(_) => *self,
        }
    }
}

impl IpEndPointExtensions for SocketAddr {
    fn un_map(&self) -> SocketAddr {
        let unmapped_addr = self.ip().un_map();
        SocketAddr::new(unmapped_addr, self.port())
    }
}
