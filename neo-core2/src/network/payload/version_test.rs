use std::vec::Vec;
use std::string::String;

use crate::testserdes;
use crate::config::netmode;
use crate::network::capability;
use assert::assert_eq;

#[test]
fn test_version_encode_decode() {
    let magic: netmode::Magic = 56753;
    let tcp_port: u16 = 3000;
    let ws_port: u16 = 3001;
    let id: u32 = 13337;
    let useragent = "/NEO:0.0.1/".to_string();
    let height: u32 = 100500;
    let capabilities = vec![
        capability::Capability {
            type_: capability::CapabilityType::TCPServer,
            data: capability::CapabilityData::Server(capability::Server {
                port: tcp_port,
            }),
        },
        capability::Capability {
            type_: capability::CapabilityType::WSServer,
            data: capability::CapabilityData::Server(capability::Server {
                port: ws_port,
            }),
        },
        capability::Capability {
            type_: capability::CapabilityType::FullNode,
            data: capability::CapabilityData::Node(capability::Node {
                start_height: height,
            }),
        },
    ];

    let version = Version::new(magic, id, useragent.clone(), capabilities.clone());
    let mut version_decoded = Version::default();
    testserdes::encode_decode_binary(&version, &mut version_decoded);

    assert_eq!(version_decoded.nonce, id);
    assert_eq!(version_decoded.capabilities, capabilities);
    assert_eq!(version_decoded.user_agent, useragent.into_bytes());
    assert_eq!(version, version_decoded);
}
