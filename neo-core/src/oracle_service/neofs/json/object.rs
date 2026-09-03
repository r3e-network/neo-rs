#[cfg(feature = "neofs-grpc")]
mod header;
#[cfg(feature = "neofs-grpc")]
mod ids;
#[cfg(feature = "neofs-grpc")]
mod payload;
#[cfg(feature = "neofs-grpc")]
mod signature;
#[cfg(feature = "neofs-grpc")]
mod version;

#[cfg(feature = "neofs-grpc")]
pub(crate) use header::neofs_json_header;
#[cfg(feature = "neofs-grpc")]
pub(crate) use ids::{neofs_json_container_id, neofs_json_object_id, neofs_json_owner_id};
#[cfg(feature = "neofs-grpc")]
pub(crate) use payload::build_neofs_object_payload;
#[cfg(feature = "neofs-grpc")]
pub(crate) use signature::neofs_json_signature;
#[cfg(feature = "neofs-grpc")]
pub(crate) use version::neofs_json_version;
