use super::super::super::NeoFsRequest;
use super::super::super::decode_raw_base58;
use super::super::super::proto::neofs_v2;

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
    decode_raw_base58(value, Some(expected_len)).ok_or_else(|| "invalid neofs id".to_string())
}
