//! SSRF protection and URL validation for Oracle service.

use std::net::IpAddr;

/// Checks if a host is an internal/private host that should be blocked.
pub(super) async fn is_internal_host(uri: &url::Url) -> Result<bool, std::io::Error> {
    let host = match uri.host_str() {
        Some(host) => host,
        None => return Ok(false),
    };

    // Check for common localhost names
    if is_localhost_name(host) {
        return Ok(true);
    }

    // Check if it's a raw IP address
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(is_internal_ip(ip));
    }

    // DNS lookup and check resolved IP
    let addr = tokio::net::lookup_host((host, 0)).await?.next();
    if let Some(addr) = addr {
        if is_internal_ip(addr.ip()) {
            return Ok(true);
        }
    }

    Ok(false)
}

/// Check if a hostname is a localhost variant.
fn is_localhost_name(host: &str) -> bool {
    let host_lower = host.to_ascii_lowercase();
    matches!(
        host_lower.as_str(),
        "localhost"
            | "localhost.localdomain"
            | "ip6-localhost"
            | "ip6-loopback"
            | "*.local"
            | "*.internal"
    ) || host_lower.ends_with(".local")
        || host_lower.ends_with(".internal")
}

/// Check if an IP address is internal/private.
pub(crate) fn is_internal_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            if ip.is_loopback() || ip.is_broadcast() || ip.is_unspecified() {
                return true;
            }
            let octets = ip.octets();
            match octets[0] {
                0 => true,      // 0.0.0.0/8 (current network)
                10 => true,     // 10.0.0.0/8 (private)
                127 => true,    // 127.0.0.0/8 (loopback)
                169 if octets[1] == 254 => true, // 169.254.0.0/16 (link-local)
                172 if (16..32).contains(&octets[1]) => true, // 172.16.0.0/12 (private)
                192 => match octets[1] {
                    0 if octets[2] == 0 || octets[2] == 2 => true, // 192.0.0.0/24, 192.0.2.0/24 (test)
                    88 if octets[2] == 99 => true, // 192.88.99.0/24 (6to4 relay)
                    168 => true, // 192.168.0.0/16 (private)
                    _ => false,
                },
                198 if octets[1] == 18 => true, // 198.18.0.0/15 (benchmark)
                198 if (51..=100).contains(&octets[1]) => true, // 198.51.100.0/24, 203.0.113.0/24 (test)
                203 if octets[1] == 0 && octets[2] == 113 => true, // 203.0.113.0/24 (test)
                224..=239 => true, // 224.0.0.0/4 (multicast)
                240..=255 => true, // 240.0.0.0/4 (reserved)
                _ => false,
            }
        }
        IpAddr::V6(ip) => {
            if ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ((ip.segments()[0] & 0xfe00) == 0xfc00)
            {
                // fc00::/7 (unique local)
                return true;
            }
            // Check for IPv4-mapped addresses
            if let Some(ipv4) = ip.to_ipv4_mapped() {
                return is_internal_ip(IpAddr::V4(ipv4));
            }
            false
        }
    }
}

/// Validates a URL for SSRF protection.
/// Returns Ok(()) if the URL is safe, Err with reason otherwise.
pub fn validate_url_for_ssrf(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;

    // Validate scheme
    let scheme = parsed.scheme();
    if !matches!(scheme, "http" | "https") {
        return Err(format!("Unsupported scheme: {}", scheme));
    }

    // Check for credentials in URL (potential security risk)
    if parsed.username() != "" || parsed.password().is_some() {
        return Err("URLs with credentials are not allowed".to_string());
    }

    // Check for non-standard ports
    if let Some(port) = parsed.port() {
        if port == 0 || port > 65535 {
            return Err("Invalid port number".to_string());
        }
        // Block common internal service ports
        if matches!(port, 22 | 23 | 25 | 53 | 110 | 143 | 993 | 995 | 3306 | 5432 | 6379 | 27017 | 9200 | 9300)
        {
            return Err("Port not allowed for security reasons".to_string());
        }
    }

    // Check host for common SSRF bypasses
    if let Some(host) = parsed.host_str() {
        // Block IPv4-mapped IPv6 addresses
        if host.starts_with("[::ffff:") || host.starts_with("[0:0:0:0:0:ffff:") {
            return Err("IPv4-mapped IPv6 addresses are not allowed".to_string());
        }

        // Block URL-encoded hosts
        if host.contains('%') {
            return Err("URL-encoded hosts are not allowed".to_string());
        }

        // Block hosts that look like IP addresses with leading zeros (octal bypass)
        if host.split('.').any(|part| {
            part.len() > 1 && part.starts_with('0') && part.chars().all(|c| c.is_ascii_digit())
        }) {
            return Err("IP addresses with octal notation are not allowed".to_string());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_internal_ip_v4() {
        assert!(is_internal_ip("127.0.0.1".parse().unwrap()));
        assert!(is_internal_ip("10.0.0.1".parse().unwrap()));
        assert!(is_internal_ip("192.168.1.1".parse().unwrap()));
        assert!(is_internal_ip("172.16.0.1".parse().unwrap()));
        assert!(is_internal_ip("0.0.0.0".parse().unwrap()));
        assert!(!is_internal_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_internal_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_is_internal_ip_v6() {
        assert!(is_internal_ip("::1".parse().unwrap()));
        assert!(is_internal_ip("::".parse().unwrap()));
        assert!(is_internal_ip("fc00::1".parse().unwrap()));
        assert!(is_internal_ip("fe80::1".parse().unwrap()));
        assert!(!is_internal_ip("2001:4860:4860::8888".parse().unwrap()));
    }

    #[test]
    fn test_is_localhost_name() {
        assert!(is_localhost_name("localhost"));
        assert!(is_localhost_name("LOCALHOST"));
        assert!(is_localhost_name("localhost.localdomain"));
        assert!(is_localhost_name("myhost.local"));
        assert!(!is_localhost_name("example.com"));
    }

    #[test]
    fn test_validate_url_for_ssrf() {
        assert!(validate_url_for_ssrf("https://example.com").is_ok());
        assert!(validate_url_for_ssrf("http://example.com/path").is_ok());
        assert!(validate_url_for_ssrf("ftp://example.com").is_err());
        assert!(validate_url_for_ssrf("https://user:pass@example.com").is_err());
    }
}
