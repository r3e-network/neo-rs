//! Rust port of `Neo.Plugins.RestServer.Models.Token.NEP11TokenModel`.

use super::nep17_token_model::Nep17TokenModel;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Extends the NEP-17 token model with token-specific metadata for NEP-11 contracts.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct Nep11TokenModel {
    #[serde(flatten)]
    pub base: Nep17TokenModel,
    /// Token-specific metadata keyed by token identifier (hex string).
    pub tokens: BTreeMap<String, Option<BTreeMap<String, Value>>>,
}

impl Nep11TokenModel {
    pub fn new(
        base: Nep17TokenModel,
        tokens: BTreeMap<String, Option<BTreeMap<String, Value>>>,
    ) -> Self {
        Self { base, tokens }
    }
}
