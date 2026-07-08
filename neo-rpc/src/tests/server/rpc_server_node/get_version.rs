use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

const LEDGER_PREFIX_CURRENT_BLOCK: u8 = 12;
const POLICY_PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
const POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
const POLICY_PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

fn serve_getversion_once(protocol: Value) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
    let url = format!("http://{}", listener.local_addr().expect("addr"));
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let read = stream.read(&mut buf).expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buf[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        let text = String::from_utf8_lossy(&request);
        assert!(
            text.contains(r#""method":"getversion""#) || text.contains(r#""method": "getversion""#),
            "unexpected request: {text}"
        );
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "tcpport": 10333,
                "nonce": 1,
                "useragent": "/remote/",
                "rpc": {
                    "maxiteratorresultitems": 123,
                    "sessionenabled": true
                },
                "protocol": protocol
            }
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
        stream.write_all(body.as_bytes()).expect("write body");
    });
    url
}

#[test]
fn get_version_dynamic_policy_reads_use_node_native_provider_boundary() {
    let version = include_str!("../../../server/rpc_server_node/version.rs");
    assert!(
        version.contains("NativeNodeProviderFactory"),
        "getversion should obtain dynamic Policy values through the node native provider factory"
    );
    assert!(
        !version.contains("StorageLedgerProviderFactory"),
        "getversion response assembly should not perform raw ledger storage reads directly"
    );
    assert!(
        !version.contains("StorageKey::new("),
        "getversion response assembly should not hand-roll native storage keys directly"
    );
    assert!(
        !version.contains("LedgerContract::"),
        "getversion response assembly should not depend on concrete LedgerContract storage"
    );
    assert!(
        !version.contains("PolicyContract::"),
        "getversion response assembly should not depend on concrete PolicyContract storage"
    );

    let provider = include_str!("../../../server/rpc_server_node/native_provider.rs");
    assert!(provider.contains("trait NodeNativeProvider"));
    assert!(provider.contains("trait NodeNativeProviderFactory"));
    assert!(provider.contains("struct NativeNodeProviderFactory"));
    assert!(
        provider.contains("ledger_queries::current_index"),
        "node native provider should read getversion's current height through the shared ledger-query boundary"
    );
    assert!(
        !provider.contains("StorageLedgerProviderFactory"),
        "node native provider should not construct the storage ledger provider directly for current-height reads"
    );
    assert!(
        provider.contains("PolicyContract::ID"),
        "node native provider should own getversion's Policy storage key boundary"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_contains_expected_fields() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    let json = result.as_object().expect("version object");

    assert!(json.get("tcpport").is_some());
    assert!(json.get("nonce").is_some());
    assert!(json.get("useragent").is_some());

    let rpc = json
        .get("rpc")
        .and_then(Value::as_object)
        .expect("rpc object");
    assert!(rpc.get("maxiteratorresultitems").is_some());
    assert!(rpc.get("sessionenabled").is_some());

    let protocol = json
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    for key in [
        "addressversion",
        "network",
        "validatorscount",
        "msperblock",
        "maxtraceableblocks",
        "maxvaliduntilblockincrement",
        "maxtransactionsperblock",
        "memorypoolmaxtransactions",
        "initialgasdistribution",
        "standbycommittee",
        "seedlist",
        "hardforks",
    ] {
        assert!(
            protocol.get(key).is_some(),
            "missing protocol field {}",
            key
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_hardforks_structure() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    let json = result.as_object().expect("version object");
    let protocol = json
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    let hardforks = protocol
        .get("hardforks")
        .and_then(Value::as_array)
        .expect("hardforks array");

    for fork in hardforks {
        let fork_obj = fork.as_object().expect("hardfork object");
        let name = fork_obj
            .get("name")
            .and_then(Value::as_str)
            .expect("hardfork name");
        let blockheight = fork_obj
            .get("blockheight")
            .and_then(Value::as_u64)
            .expect("hardfork blockheight");
        assert!(!name.starts_with("HF_"));
        let _ = blockheight;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_includes_zero_height_hardforks() {
    let mut settings = ProtocolSettings::default();
    for height in settings.hardforks.values_mut() {
        *height = 0;
    }
    let expected = settings.hardforks.len();
    let system = crate::server::test_support::test_system(settings);
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    let json = result.as_object().expect("version object");
    let protocol = json
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    let hardforks = protocol
        .get("hardforks")
        .and_then(Value::as_array)
        .expect("hardforks array");
    assert_eq!(hardforks.len(), expected);
    assert!(hardforks.iter().all(|fork| {
        fork.as_object()
            .and_then(|obj| obj.get("blockheight"))
            .and_then(Value::as_u64)
            == Some(0)
    }));
}

/// Settings with every hardfork (including HF_Echidna) active from
/// height 0, so the genesis-seeded fixture (current index 0) takes the
/// dynamic post-Echidna read path.
fn echidna_at_zero_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    for height in settings.hardforks.values_mut() {
        *height = 0;
    }
    settings
}

/// Writes the three committee-adjustable Policy values the dynamic
/// getversion path reads, in the storage encoding the Policy setters
/// use (signed little-endian `BigInteger` bytes).
fn seed_policy_dynamic_values(
    system: &std::sync::Arc<crate::server::NodeContext>,
    msperblock: u32,
    max_traceable: u32,
    max_vub_increment: u32,
) {
    let mut store = system.store_cache();
    for (prefix, value) in [
        (POLICY_PREFIX_MILLISECONDS_PER_BLOCK, msperblock),
        (POLICY_PREFIX_MAX_TRACEABLE_BLOCKS, max_traceable),
        (
            POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT,
            max_vub_increment,
        ),
    ] {
        store.update(
            StorageKey::new(PolicyContract::ID, vec![prefix]),
            StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()),
        );
    }
    store.commit();
}

/// Reads the (msperblock, maxtraceableblocks, maxvaliduntilblockincrement)
/// triple from a getversion result.
fn version_dynamic_triple(result: &Value) -> (u64, u64, u64) {
    let protocol = result
        .get("protocol")
        .and_then(Value::as_object)
        .expect("protocol object");
    let read = |key: &str| {
        protocol
            .get(key)
            .and_then(Value::as_u64)
            .unwrap_or_else(|| panic!("numeric protocol field {key}"))
    };
    (
        read("msperblock"),
        read("maxtraceableblocks"),
        read("maxvaliduntilblockincrement"),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_reads_policy_storage_when_echidna_active() {
    // C# NeoSystemExtensions: with HF_Echidna enabled at the current
    // height, the values come from native Policy storage.
    let settings = echidna_at_zero_settings();
    let system = crate::server::test_support::test_system(settings);
    seed_policy_dynamic_values(&system, 3_000, 1_000_000, 1_024);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    assert_eq!(version_dynamic_triple(&result), (3_000, 1_000_000, 1_024));
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_ledger_get_version_uses_upstream_policy_values() {
    let system = crate::server::test_support::test_system(echidna_at_zero_settings());
    let mut server = RpcServer::new(system, RpcServerConfig::default());
    server
        .set_remote_ledger_rpc(serve_getversion_once(json!({
            "msperblock": 3_000,
            "maxtraceableblocks": 1_000_000,
            "maxvaliduntilblockincrement": 1_024,
        })))
        .expect("configure remote ledger");
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");

    assert_eq!(version_dynamic_triple(&result), (3_000, 1_000_000, 1_024));
    assert_ne!(
        result
            .get("useragent")
            .and_then(Value::as_str)
            .expect("local useragent"),
        "/remote/",
        "remote-ledger mode should keep this node's identity fields local"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_falls_back_to_settings_when_policy_storage_absent() {
    // C# NeoSystemExtensions: Policy.Get* throws KeyNotFoundException
    // when the Echidna-era keys were never written (e.g. Echidna active
    // from height 0 before its activation migration ran); the catch
    // block falls back to the static ProtocolSettings values.
    let settings = echidna_at_zero_settings();
    let expected = (
        u64::from(settings.milliseconds_per_block),
        u64::from(settings.max_traceable_blocks),
        u64::from(settings.max_valid_until_block_increment),
    );
    let system = crate::server::test_support::test_system(settings);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    assert_eq!(version_dynamic_triple(&result), expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_reports_static_settings_before_echidna() {
    // C# NeoSystemExtensions: before HF_Echidna the static settings
    // win even if Policy storage carries (stale/foreign) values.
    let settings = ProtocolSettings::default(); // mainnet Echidna height >> 0
    let expected = (
        u64::from(settings.milliseconds_per_block),
        u64::from(settings.max_traceable_blocks),
        u64::from(settings.max_valid_until_block_increment),
    );
    let system = crate::server::test_support::test_system(settings);
    seed_policy_dynamic_values(&system, 3_000, 1_000_000, 1_024);

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    assert_eq!(version_dynamic_triple(&result), expected);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_version_falls_back_to_settings_when_ledger_pointer_absent() {
    // C# NeoSystemExtensions: Ledger.CurrentIndex throws
    // KeyNotFoundException while genesis is not yet persisted, and the
    // catch block falls back to settings — even with Echidna at 0 and
    // Policy values present.
    let settings = echidna_at_zero_settings();
    let expected = (
        u64::from(settings.milliseconds_per_block),
        u64::from(settings.max_traceable_blocks),
        u64::from(settings.max_valid_until_block_increment),
    );
    let system = crate::server::test_support::test_system(settings);
    seed_policy_dynamic_values(&system, 3_000, 1_000_000, 1_024);
    let mut store = system.store_cache();
    store.delete(StorageKey::new(
        LedgerContract::ID,
        vec![LEDGER_PREFIX_CURRENT_BLOCK],
    ));
    store.commit();

    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getversion");

    let result = (handler.callback())(&server, &[]).expect("get version");
    assert_eq!(version_dynamic_triple(&result), expected);
}
