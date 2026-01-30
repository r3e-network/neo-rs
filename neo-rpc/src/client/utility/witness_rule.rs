use neo_core::config::ProtocolSettings;
use neo_core::{ECCurve, ECPoint, WitnessCondition, WitnessRule, WitnessRuleAction};
use neo_json::{JArray, JObject};

pub fn rule_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> Result<WitnessRule, String> {
    let action_str = json
        .get("action")
        .and_then(neo_json::JToken::as_string)
        .ok_or_else(|| "WitnessRule missing action".to_string())?;
    let action: WitnessRuleAction = action_str.parse()?;
    let condition_token = json
        .get("condition")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "WitnessRule missing condition".to_string())?;
    let condition = condition_from_json(
        condition_token,
        protocol_settings,
        WitnessCondition::MAX_NESTING_DEPTH,
    )?;
    Ok(WitnessRule::new(action, condition))
}

fn condition_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
    max_depth: usize,
) -> Result<WitnessCondition, String> {
    if max_depth == 0 {
        return Err("Max nesting depth exceeded".to_string());
    }

    let condition_type = json
        .get("type")
        .and_then(neo_json::JToken::as_string)
        .ok_or_else(|| "Condition type missing".to_string())?;

    match condition_type.as_str() {
        "Or" => {
            let expressions = json
                .get("expressions")
                .and_then(|value| value.as_array())
                .ok_or_else(|| "Or condition missing expressions".to_string())?;
            parse_composite(expressions, protocol_settings, max_depth, true)
        }
        "And" => {
            let expressions = json
                .get("expressions")
                .and_then(|value| value.as_array())
                .ok_or_else(|| "And condition missing expressions".to_string())?;
            parse_composite(expressions, protocol_settings, max_depth, false)
        }
        "Boolean" => {
            let expression = json
                .get("expression")
                .or_else(|| json.get("value"))
                .ok_or_else(|| "Boolean condition missing expression".to_string())?;
            Ok(WitnessCondition::Boolean {
                value: expression.as_boolean(),
            })
        }
        "Not" => {
            let expression = json
                .get("expression")
                .and_then(|value| value.as_object())
                .ok_or_else(|| "Not condition missing expression".to_string())?;
            let condition = condition_from_json(expression, protocol_settings, max_depth - 1)?;
            Ok(WitnessCondition::Not {
                condition: Box::new(condition),
            })
        }
        "Group" => {
            let group = json
                .get("group")
                .and_then(neo_json::JToken::as_string)
                .ok_or_else(|| "Group condition missing group".to_string())?;
            Ok(WitnessCondition::Group {
                group: parse_group_bytes(&group)?,
            })
        }
        "CalledByContract" => {
            let hash = json
                .get("hash")
                .and_then(neo_json::JToken::as_string)
                .ok_or_else(|| "CalledByContract missing hash".to_string())?;
            let hash = super::RpcUtility::get_script_hash(&hash, protocol_settings)?;
            Ok(WitnessCondition::CalledByContract { hash })
        }
        "ScriptHash" => {
            let hash = json
                .get("hash")
                .and_then(neo_json::JToken::as_string)
                .ok_or_else(|| "ScriptHash condition missing hash".to_string())?;
            let hash = super::RpcUtility::get_script_hash(&hash, protocol_settings)?;
            Ok(WitnessCondition::ScriptHash { hash })
        }
        "CalledByEntry" => Ok(WitnessCondition::CalledByEntry),
        "CalledByGroup" => {
            let group = json
                .get("group")
                .and_then(neo_json::JToken::as_string)
                .ok_or_else(|| "CalledByGroup missing group".to_string())?;
            Ok(WitnessCondition::CalledByGroup {
                group: parse_group_bytes(&group)?,
            })
        }
        other => Err(format!("Unsupported witness condition type: {other}")),
    }
}

fn parse_composite(
    expressions: &JArray,
    protocol_settings: &ProtocolSettings,
    max_depth: usize,
    is_or: bool,
) -> Result<WitnessCondition, String> {
    if expressions.is_empty() {
        return Err("Composite witness condition requires at least one expression".to_string());
    }
    if expressions.len() > WitnessCondition::MAX_SUBITEMS {
        return Err("Composite witness condition exceeds max subitems".to_string());
    }

    let mut conditions = Vec::with_capacity(expressions.len());
    for expr in expressions.children() {
        let expr_obj = expr
            .as_ref()
            .and_then(|value| value.as_object())
            .ok_or_else(|| "Witness condition expression must be an object".to_string())?;
        conditions.push(condition_from_json(
            expr_obj,
            protocol_settings,
            max_depth - 1,
        )?);
    }

    if is_or {
        Ok(WitnessCondition::Or { conditions })
    } else {
        Ok(WitnessCondition::And { conditions })
    }
}

fn parse_group_bytes(value: &str) -> Result<Vec<u8>, String> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    let bytes = hex::decode(value).map_err(|err| format!("Invalid hex string: {err}"))?;
    let point = ECPoint::decode(&bytes, ECCurve::secp256r1())
        .map_err(|err| format!("Invalid ECPoint: {err}"))?;
    point
        .encode_point(true)
        .map_err(|err| format!("Failed to encode ECPoint: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::{KeyPair, UInt160};
    use neo_json::JToken;

    fn assert_rule_roundtrip(rule: WitnessRule) {
        let json = rule.to_json();
        let token = JToken::parse(&json.to_string(), 128).expect("parse rule json");
        let obj = token.as_object().expect("rule object");
        let parsed =
            rule_from_json(obj, &ProtocolSettings::default_settings()).expect("rule parse");
        assert_eq!(parsed.to_json(), json);
    }

    #[test]
    fn rule_from_json_roundtrip_matches_csharp_cases() {
        let action = WitnessRuleAction::Allow;

        assert_rule_roundtrip(WitnessRule::new(action, WitnessCondition::CalledByEntry));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::Or {
                conditions: vec![
                    WitnessCondition::Boolean { value: true },
                    WitnessCondition::Boolean { value: false },
                ],
            },
        ));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::And {
                conditions: vec![
                    WitnessCondition::Boolean { value: true },
                    WitnessCondition::Boolean { value: false },
                ],
            },
        ));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::Boolean { value: true },
        ));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::Not {
                condition: Box::new(WitnessCondition::Boolean { value: true }),
            },
        ));

        let keypair = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
            .expect("keypair");
        let group = keypair.compressed_public_key();

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::Group {
                group: group.clone(),
            },
        ));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::CalledByContract {
                hash: UInt160::zero(),
            },
        ));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::ScriptHash {
                hash: UInt160::zero(),
            },
        ));

        assert_rule_roundtrip(WitnessRule::new(
            action,
            WitnessCondition::CalledByGroup { group },
        ));
    }
}
