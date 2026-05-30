use super::super::super::proto::neofs_v2;

pub fn neofs_json_version(version: &neofs_v2::refs::Version) -> String {
    format!(
        "{{ \"major\": {}, \"minor\": {} }}",
        version.major, version.minor
    )
}
