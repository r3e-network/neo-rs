use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Once;
use std::time::Duration;
use std::sync::Mutex;
use lazy_static::lazy_static;
use reqwest::Client;
use reqwest::Error;
use std::collections::HashSet;
use std::net::AddrParseError;
use std::fmt;

lazy_static! {
    static ref PRIVATE_NETS: Mutex<HashSet<IpAddr>> = Mutex::new(HashSet::new());
}

static INIT: Once = Once::new();

fn init() {
    INIT.call_once(|| {
        let reserved_cidrs = vec![
            // IPv4
            "10.0.0.0/8",
            "100.64.0.0/10",
            "172.16.0.0/12",
            "192.0.0.0/24",
            "192.168.0.0/16",
            "198.18.0.0/15",
            // IPv6
            "fc00::/7",
        ];

        let mut private_nets = PRIVATE_NETS.lock().unwrap();
        for cidr in reserved_cidrs {
            match cidr.parse::<IpAddr>() {
                Ok(ip) => { private_nets.insert(ip); },
                Err(_) => { panic!("Failed to parse CIDR: {}", cidr); }
            }
        }
    });
}

fn is_reserved(ip: IpAddr) -> bool {
    init();
    let private_nets = PRIVATE_NETS.lock().unwrap();
    !ip.is_global() || private_nets.contains(&ip)
}

#[derive(Debug)]
struct RestrictedRedirectError {
    details: String
}

impl RestrictedRedirectError {
    fn new(msg: &str) -> RestrictedRedirectError {
        RestrictedRedirectError{details: msg.to_string()}
    }
}

impl fmt::Display for RestrictedRedirectError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}

impl std::error::Error for RestrictedRedirectError {
    fn description(&self) -> &str {
        &self.details
    }
}

fn get_default_client(cfg: &config::OracleConfiguration) -> Result<Client, Box<dyn std::error::Error>> {
    let mut builder = Client::builder();
    if !cfg.allow_private_host {
        builder = builder.danger_accept_invalid_certs(true);
        builder = builder.redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 10 {
                attempt.error(RestrictedRedirectError::new("too many redirects"))
            } else if attempt.previous().len() > 0 && attempt.previous()[0].url().scheme() == "https" && attempt.url().scheme() != "https" {
                attempt.error(RestrictedRedirectError::new("redirected from https to http"))
            } else {
                attempt.follow()
            }
        }));
    }
    let client = builder.timeout(Duration::from_secs(cfg.request_timeout)).build()?;
    Ok(client)
}
