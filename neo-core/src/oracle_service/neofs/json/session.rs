#![allow(unused_imports)]

#[cfg(feature = "neofs-grpc")]
mod v1;
#[cfg(feature = "neofs-grpc")]
mod v2;

#[cfg(feature = "neofs-grpc")]
pub(crate) use v1::neofs_json_session_token;
#[cfg(feature = "neofs-grpc")]
pub(crate) use v2::neofs_json_session_token_v2;
