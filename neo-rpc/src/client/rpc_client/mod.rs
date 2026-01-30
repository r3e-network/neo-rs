// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_client/mod.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

mod builder;
mod client;
mod helpers;
mod hooks;

#[cfg(test)]
mod tests;

use regex::Regex;
use reqwest::{Client, Url};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use neo_core::config::ProtocolSettings;

pub use builder::RpcClientBuilder;
pub use hooks::{RpcClientHooks, RpcRequestOutcome};

static RPC_NAME_REGEX: OnceLock<Regex> = OnceLock::new();
const MAX_JSON_NESTING: usize = 128;
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// The RPC client to call NEO RPC methods
/// Matches C# `RpcClient`
#[derive(Clone)]
pub struct RpcClient {
    base_address: Url,
    http_client: Client,
    pub(crate) protocol_settings: Arc<ProtocolSettings>,
    request_timeout: Duration,
    hooks: RpcClientHooks,
}
