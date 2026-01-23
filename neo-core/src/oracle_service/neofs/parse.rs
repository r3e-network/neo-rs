use super::{NeoFsCommand, NeoFsRange, NeoFsRequest};

pub(super) fn parse_neofs_request(url: &str) -> Result<NeoFsRequest, String> {
    let (_, suffix) = url
        .split_once(':')
        .ok_or_else(|| "Invalid neofs url".to_string())?;
    let mut path = suffix;
    if let Some((before, _)) = path.split_once('?') {
        path = before;
    }
    if let Some((before, _)) = path.split_once('#') {
        path = before;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 2 {
        return Err("Invalid neofs url".to_string());
    }

    let container = segments[0].to_string();
    let object = segments[1].to_string();
    if container.is_empty() || object.is_empty() {
        return Err("Invalid neofs url".to_string());
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
                .ok_or_else(|| "missing object range (expected 'Offset|Length')".to_string())?;
            NeoFsCommand::Range(parse_neofs_range(range_raw)?)
        }
        "header" => NeoFsCommand::Header,
        "hash" => {
            let range = match segments.get(3) {
                Some(raw) => Some(parse_neofs_range(raw)?),
                None => None,
            };
            NeoFsCommand::Hash(range)
        }
        _ => return Err("invalid command".to_string()),
    };

    Ok(NeoFsRequest {
        container,
        object,
        command,
    })
}

pub(super) fn parse_neofs_range(raw: &str) -> Result<NeoFsRange, String> {
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map_err(|_| "object range is invalid (expected 'Offset|Length')".to_string())?;
    let (offset_str, length_str) = decoded
        .split_once('|')
        .ok_or_else(|| "object range is invalid (expected 'Offset|Length')".to_string())?;
    let offset = offset_str
        .parse::<u64>()
        .map_err(|_| "object range is invalid (expected 'Offset|Length')".to_string())?;
    let length = length_str
        .parse::<u64>()
        .map_err(|_| "object range is invalid (expected 'Offset|Length')".to_string())?;
    Ok(NeoFsRange { offset, length })
}

fn validate_neofs_id(value: &str, kind: &str) -> Result<(), String> {
    let decoded = bs58::decode(value)
        .into_vec()
        .map_err(|_| format!("invalid neofs {} id", kind))?;
    if decoded.len() != 32 {
        return Err(format!("invalid neofs {} id", kind));
    }
    Ok(())
}
