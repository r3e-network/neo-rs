use serde_json;
use std::str::FromStr;
use crate::network::PeerInfo;
use crate::result::GetPeers;

#[test]
fn test_get_peers() {
    let mut gp = GetPeers::new();
    assert_eq!(gp.unconnected.len(), 0);
    assert_eq!(gp.connected.len(), 0);
    assert_eq!(gp.bad.len(), 0);

    gp.add_unconnected(vec!["1.1.1.1:53".to_string(), "8.8.8.8:53".to_string(), "9.9.9.9:53".to_string()]);
    let unsupported_format = "2001:DB0:0:123A:::30".to_string();
    gp.add_connected(vec![
        PeerInfo { address: "192.168.0.1:10333".to_string(), user_agent: "/NEO-GO:0.106.2/".to_string(), height: 100 },
        PeerInfo { address: unsupported_format.clone(), user_agent: "".to_string(), height: 0 },
        PeerInfo { address: "[2001:DB0:0:123A::]:30".to_string(), user_agent: "/NEO-GO:0.106.2/".to_string(), height: 200 },
    ]);
    gp.add_bad(vec!["127.0.0.1:20333".to_string(), "127.0.0.1:65536".to_string()]);

    assert_eq!(gp.unconnected.len(), 3);
    assert_eq!(gp.connected.len(), 2);
    assert_eq!(gp.bad.len(), 2);
    assert_eq!(gp.connected[0].address, "192.168.0.1:10333");
    assert_eq!(gp.connected[0].port(), 10333);
    assert_eq!(gp.connected[0].user_agent, "/NEO-GO:0.106.2/");
    assert_eq!(gp.connected[0].last_known_height, 100);
    assert_eq!(gp.connected[1].port(), 30);
    assert_eq!(gp.connected[1].user_agent, "/NEO-GO:0.106.2/");
    assert_eq!(gp.connected[1].last_known_height, 200);
    assert_eq!(gp.bad[0].address, "127.0.0.1:20333");
    assert_eq!(gp.bad[0].port(), 20333);

    let mut gps = GetPeers::new();
    let old_peer_format = r#"{"unconnected": [{"address": "20.109.188.128","port": "10333"},{"address": "27.188.182.47","port": "10333"}],"connected": [{"address": "54.227.43.72","port": "10333"},{"address": "157.90.177.38","port": "10333"}],"bad": [{"address": "5.226.142.226","port": "10333"}]}"#;
    let err = serde_json::from_str::<GetPeers>(old_peer_format);
    assert!(err.is_ok());
    gps = err.unwrap();

    let new_peer_format = r#"{"unconnected": [{"address": "20.109.188.128","port": 10333},{"address": "27.188.182.47","port": 10333}],"connected": [{"address": "54.227.43.72","port": 10333},{"address": "157.90.177.38","port": 10333}],"bad": [{"address": "5.226.142.226","port": 10333},{"address": "54.208.117.178","port": 10333}]}"#;
    let err = serde_json::from_str::<GetPeers>(new_peer_format);
    assert!(err.is_ok());
    gps = err.unwrap();

    let bad_int_format = r#"{"unconnected": [{"address": "20.109.188.128","port": 65536}],"connected": [],"bad": []}"#;
    let err = serde_json::from_str::<GetPeers>(bad_int_format);
    assert!(err.is_err());

    let bad_string_format = r#"{"unconnected": [{"address": "20.109.188.128","port": "badport"}],"connected": [],"bad": []}"#;
    let err = serde_json::from_str::<GetPeers>(bad_string_format);
    assert!(err.is_err());
}
