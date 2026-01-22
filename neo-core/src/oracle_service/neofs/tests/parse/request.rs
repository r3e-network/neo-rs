use super::super::super::parse::parse_neofs_request;
use super::super::super::NeoFsCommand;

fn sample_neofs_id(byte: u8) -> String {
    bs58::encode([byte; 32]).into_string()
}

#[test]
fn parse_neofs_request_payload() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let request =
        parse_neofs_request(&format!("neofs:{}/{}", container, object))
            .expect("parse payload");
    assert_eq!(request.container, container);
    assert_eq!(request.object, object);
    assert!(matches!(request.command, NeoFsCommand::Payload));
}

#[test]
fn parse_neofs_request_rejects_authority_urls() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let err = parse_neofs_request(&format!("neofs://{}/{}", container, object))
        .expect_err("authority-style URL should fail");
    assert!(err.contains("Invalid neofs url"));
}

#[test]
fn parse_neofs_request_rejects_double_slash() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let err = parse_neofs_request(&format!("neofs:{container}//{object}"))
        .expect_err("double slash should fail");
    assert!(err.contains("Invalid neofs url"));
}

#[test]
fn parse_neofs_request_range() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let request = parse_neofs_request(&format!(
        "neofs:{}/{}/range/10|20",
        container, object
    ))
    .expect("parse range");
    assert_eq!(request.container, container);
    assert_eq!(request.object, object);
    match request.command {
        NeoFsCommand::Range(range) => {
            assert_eq!(range.offset, 10);
            assert_eq!(range.length, 20);
        }
        _ => panic!("expected range command"),
    }
}

#[test]
fn parse_neofs_request_range_percent_decoded() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let request = parse_neofs_request(&format!(
        "neofs:{}/{}/range/10%7C20",
        container, object
    ))
    .expect("parse range");
    match request.command {
        NeoFsCommand::Range(range) => {
            assert_eq!(range.offset, 10);
            assert_eq!(range.length, 20);
        }
        _ => panic!("expected range command"),
    }
}

#[test]
fn parse_neofs_request_header_and_hash() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let header = parse_neofs_request(&format!("neofs:{}/{}/header", container, object))
        .expect("parse header");
    assert!(matches!(header.command, NeoFsCommand::Header));

    let hash =
        parse_neofs_request(&format!("neofs:{}/{}/hash", container, object))
            .expect("parse hash");
    match hash.command {
        NeoFsCommand::Hash(None) => {}
        _ => panic!("expected hash without range"),
    }

    let hash_range = parse_neofs_request(&format!(
        "neofs:{}/{}/hash/5|7",
        container, object
    ))
    .expect("parse hash range");
    match hash_range.command {
        NeoFsCommand::Hash(Some(range)) => {
            assert_eq!(range.offset, 5);
            assert_eq!(range.length, 7);
        }
        _ => panic!("expected hash with range"),
    }
}

#[test]
fn parse_neofs_request_ignores_query_fragment() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let request = parse_neofs_request(&format!(
        "neofs:{}/{}/header?foo=1#bar",
        container, object
    ))
    .expect("parse header with query");
    assert!(matches!(request.command, NeoFsCommand::Header));
    assert_eq!(request.container, container);
    assert_eq!(request.object, object);
}

#[test]
fn parse_neofs_request_missing_range_errors() {
    let container = sample_neofs_id(1);
    let object = sample_neofs_id(2);
    let err = parse_neofs_request(&format!("neofs:{}/{}/range", container, object))
        .expect_err("range should error");
    assert!(
        err.contains("missing object range"),
        "unexpected error: {err}"
    );
}

#[test]
fn parse_neofs_request_rejects_invalid_ids() {
    let object = sample_neofs_id(2);
    let err =
        parse_neofs_request(&format!("neofs:0/{}", object)).expect_err("invalid id");
    assert!(
        err.contains("invalid neofs container id"),
        "unexpected error: {err}"
    );
}
