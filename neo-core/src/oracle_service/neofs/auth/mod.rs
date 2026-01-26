#![allow(dead_code)]

mod bearer;
mod signing;

pub(crate) use bearer::{build_neofs_auth, strip_bearer_prefix};
#[allow(dead_code)]
pub(crate) use signing::{salt_message_wallet_connect, sign_neofs_sha512};
