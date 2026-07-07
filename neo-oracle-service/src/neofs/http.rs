// Rationale: NeoFS HTTP integration keeps builder/request helpers compiled as
// a feature-ready adapter even when only part of the adapter is wired at runtime.
#![allow(dead_code)]

mod builder;
mod requests;
mod utils;

// Rationale: normalization helpers are exported for both HTTP and optional
// NeoFS integration paths, which may be feature-gated by deployment.
#[allow(unused)]
pub(crate) use utils::{map_neofs_status, normalize_neofs_endpoint};
