// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of `Neo.Plugins.RestServer.Newtonsoft.Json.WitnessRuleJsonConverter`.

use crate::rest_server::rest_server_utility::RestServerUtility;
use neo_core::witness_rule::WitnessRule;
use serde_json::Value;

pub struct WitnessRuleJsonConverter;

impl WitnessRuleJsonConverter {
    pub fn to_json(rule: &WitnessRule) -> Value {
        RestServerUtility::witness_rule_to_j_token(rule)
    }
}
