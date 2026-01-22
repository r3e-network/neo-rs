use super::super::super::proto::neofs_v2;
use super::super::super::NeoFsRequest;

pub(crate) fn build_neofs_grpc_address(
    request: &NeoFsRequest,
) -> Result<neofs_v2::refs::Address, String> {
    let container = decode_neofs_id_bytes(&request.container, 32)?;
    let object = decode_neofs_id_bytes(&request.object, 32)?;
    Ok(neofs_v2::refs::Address {
        container_id: Some(neofs_v2::refs::ContainerId { value: container }),
        object_id: Some(neofs_v2::refs::ObjectId { value: object }),
    })
}

fn decode_neofs_id_bytes(value: &str, expected_len: usize) -> Result<Vec<u8>, String> {
    let decoded = bs58::decode(value)
        .into_vec()
        .map_err(|_| "invalid neofs id".to_string())?;
    if decoded.len() != expected_len {
        return Err("invalid neofs id".to_string());
    }
    Ok(decoded)
}
