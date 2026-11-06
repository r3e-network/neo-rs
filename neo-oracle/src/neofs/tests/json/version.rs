#[cfg(feature = "neofs-grpc")]
use super::super::super::json::{neofs_json_object_id, neofs_json_version};
#[cfg(feature = "neofs-grpc")]
use super::super::super::proto::neofs_v2;

#[cfg(feature = "neofs-grpc")]
#[test]
fn neofs_json_empty_message_formats_as_empty_object() {
    let version = neofs_v2::refs::Version { major: 0, minor: 0 };
    let json = neofs_json_version(&version);
    assert_eq!(json, r#"{ "major": 0, "minor": 0 }"#);

    let empty_id = neofs_v2::refs::ObjectId { value: Vec::new() };
    let json = neofs_json_object_id(&empty_id);
    assert_eq!(json, Some(r#"{ "value": "" }"#.to_string()));
}
