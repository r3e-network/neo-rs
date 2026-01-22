pub(super) async fn is_internal_host(uri: &url::Url) -> Result<bool, std::io::Error> {
    let host = match uri.host_str() {
        Some(host) => host,
        None => return Ok(false),
    };
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(is_internal_ip(ip));
    }

    let addr = tokio::net::lookup_host((host, 0)).await?.next();
    if let Some(addr) = addr {
        if is_internal_ip(addr.ip()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn is_internal_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ip) => {
            if ip.is_loopback() || ip.is_broadcast() || ip.is_unspecified() {
                return true;
            }
            let octets = ip.octets();
            match octets[0] {
                10 | 127 => true,
                172 => (16..32).contains(&octets[1]),
                192 => octets[1] == 168,
                _ => false,
            }
        }
        std::net::IpAddr::V6(ip) => ip.is_loopback() || ip.is_unspecified(),
    }
}
