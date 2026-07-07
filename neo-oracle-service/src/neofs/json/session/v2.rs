// Rationale: NeoFS v2 session JSON mirrors optional protobuf fields that are
// not all exercised by the current oracle request path.
#![allow(dead_code)]

mod context;
mod token;
mod verbs;

// Rationale: the v2 token projector is exported for the optional NeoFS gRPC
// path and can be unused in HTTP-only builds.
#[allow(unused)]
pub(crate) use token::neofs_json_session_token_v2;
