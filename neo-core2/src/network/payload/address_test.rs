use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;

use crate::network::payload::{AddressAndTime, AddressList, NewAddressAndTime, NewAddressList, MaxAddrsCount};
use crate::network::capability::{Capability, Capabilities, Server, TCPServer};
use crate::testserdes;
use assert2::{assert, let_assert};
use anyhow::Result;

#[test]
fn test_encode_decode_address() -> Result<()> {
    let e = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2000);
    let ts = SystemTime::now();
    let addr = NewAddressAndTime(e, ts, Capabilities(vec![
        Capability {
            Type: TCPServer,
            Data: Arc::new(Server { Port: e.port() }),
        },
    ]));

    assert!(ts.duration_since(UNIX_EPOCH)?.as_secs() as i64 == addr.timestamp as i64);

    // On Windows or macOS localhost can be resolved to 4-bytes IPv4.
    let mut expected = [0u8; 16];
    expected[..4].copy_from_slice(&e.ip().to_string().as_bytes());

    let mut aat_ip = [0u8; 16];
    aat_ip[..4].copy_from_slice(&addr.ip.to_string().as_bytes());

    assert!(expected == aat_ip);
    assert!(addr.capabilities.len() == 1);
    assert!(addr.capabilities[0] == Capability {
        Type: TCPServer,
        Data: Arc::new(Server { Port: e.port() }),
    });

    testserdes::encode_decode_binary(&addr, &mut AddressAndTime::default())?;
    Ok(())
}

fn fill_address_list(al: &mut AddressList) -> Result<()> {
    for (i, addr) in al.addrs.iter_mut().enumerate() {
        let e = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 2000 + i as u16);
        *addr = NewAddressAndTime(e, SystemTime::now(), Capabilities(vec![
            Capability {
                Type: TCPServer,
                Data: Arc::new(Server { Port: 123 }),
            },
        ]));
    }
    Ok(())
}

#[test]
fn test_encode_decode_address_list() -> Result<()> {
    let len_list: u8 = 4;
    let mut addr_list = NewAddressList(len_list as usize);
    fill_address_list(&mut addr_list)?;
    testserdes::encode_decode_binary(&addr_list, &mut AddressList::default())?;
    Ok(())
}

#[test]
fn test_encode_decode_bad_address_list() -> Result<()> {
    let mut new_al = AddressList::default();
    let mut addr_list = NewAddressList(MaxAddrsCount + 1);
    fill_address_list(&mut addr_list)?;

    let bin = testserdes::encode_binary(&addr_list)?;
    assert!(testserdes::decode_binary(&bin, &mut new_al).is_err());

    addr_list = NewAddressList(0);
    let bin = testserdes::encode_binary(&addr_list)?;
    assert!(testserdes::decode_binary(&bin, &mut new_al).is_err());
    Ok(())
}

#[test]
fn test_get_tcp_address() -> Result<()> {
    let mut p = AddressAndTime::default();
    p.ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
    p.capabilities.push(Capability {
        Type: TCPServer,
        Data: Arc::new(Server { Port: 123 }),
    });
    let (s, err) = p.get_tcp_address();
    assert!(err.is_none());
    assert!(s == "1.1.1.1:123");

    let mut p = AddressAndTime::default();
    let (s, err) = p.get_tcp_address();
    println!("{:?}, {:?}", s, err);
    Ok(())
}
