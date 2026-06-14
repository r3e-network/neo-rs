use super::{NeoFsCommand, NeoFsRange, NeoFsRequest, decode_raw_base58};
use neo_error::{CoreError, CoreResult};

impl NeoFsRequest {
pub(super) fn parse_neofs_request(url: &str) -> CoreResult<NeoFsRequest> {
    let (_, suffix) = url
        .split_once(':')
        .ok_or_else(|| CoreError::other("Invalid neofs url"))?;
    let mut path = suffix;
    if let Some((before, _)) = path.split_once('?') {
        path = before;
    }
    if let Some((before, _)) = path.split_once('#') {
        path = before;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 2 {
        return Err(CoreError::other("Invalid neofs url"));
    }

    let container = segments[0].to_string();
    let object = segments[1].to_string();
    if container.is_empty() || object.is_empty() {
        return Err(CoreError::other("Invalid neofs url"));
    }
    validate_neofs_id(&container, "container")?;
    validate_neofs_id(&object, "object")?;

    if segments.len() == 2 {
        return Ok(NeoFsRequest {
            container,
            object,
            command: NeoFsCommand::Payload,
        });
    }

    let command = segments[2];
    let command = match command {
        "range" => {
            let range_raw = segments
                .get(3)
                .ok_or_else(|| CoreError::other("missing object range (expected 'Offset|Length')"))?;
            NeoFsCommand::Range(NeoFsRange::parse_neofs_range(range_raw)?)
        }
        "header" => NeoFsCommand::Header,
        "hash" => {
            let range = match segments.get(3) {
                Some(raw) => Some(NeoFsRange::parse_neofs_range(raw)?),
                None => None,
            };
            NeoFsCommand::Hash(range)
        }
        _ => return Err(CoreError::other("invalid command")),
    };

    Ok(NeoFsRequest {
        container,
        object,
        command,
    })
}
}

impl NeoFsRange {
pub(super) fn parse_neofs_range(raw: &str) -> CoreResult<NeoFsRange> {
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map_err(|_| CoreError::other("object range is invalid (expected 'Offset|Length')"))?;
    let (offset_str, length_str) = decoded
        .split_once('|')
        .ok_or_else(|| CoreError::other("object range is invalid (expected 'Offset|Length')"))?;
    let offset = offset_str
        .parse::<u64>()
        .map_err(|_| CoreError::other("object range is invalid (expected 'Offset|Length')"))?;
    let length = length_str
        .parse::<u64>()
        .map_err(|_| CoreError::other("object range is invalid (expected 'Offset|Length')"))?;
    Ok(NeoFsRange { offset, length })
}
}

fn validate_neofs_id(value: &str, kind: &str) -> CoreResult<()> {
    if decode_raw_base58(value, Some(32)).is_none() {
        return Err(CoreError::other(format!("invalid neofs {} id", kind)));
    }
    Ok(())
}
