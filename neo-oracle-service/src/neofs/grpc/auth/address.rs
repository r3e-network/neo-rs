use super::super::super::NeoFsRequest;
use super::super::super::decode_raw_base58;
use super::super::super::proto::neofs_v2;
use neo_error::{CoreError, CoreResult};

pub(crate) fn build_neofs_grpc_address(
    request: &NeoFsRequest,
) -> CoreResult<neofs_v2::refs::Address> {
    let container = decode_neofs_id_bytes(&request.container, 32)?;
    let object = decode_neofs_id_bytes(&request.object, 32)?;
    Ok(neofs_v2::refs::Address {
        container_id: Some(neofs_v2::refs::ContainerId { value: container }),
        object_id: Some(neofs_v2::refs::ObjectId { value: object }),
    })
}

fn decode_neofs_id_bytes(value: &str, expected_len: usize) -> CoreResult<Vec<u8>> {
    decode_raw_base58(value, Some(expected_len))
        .ok_or_else(|| CoreError::other("invalid neofs id"))
}
