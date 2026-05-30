use super::super::super::proto::neofs_v2;

pub fn build_neofs_object_payload(
    header: &neofs_v2::object::Header,
    data: &[u8],
) -> neofs_v2::object::Object {
    neofs_v2::object::Object {
        object_id: None,
        signature: None,
        header: Some(header.clone()),
        payload: data.to_vec(),
    }
}
