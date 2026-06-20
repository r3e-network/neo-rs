use neo_config::ProtocolSettings;
use neo_crypto::{ECCurve, ECPoint};
use neo_error::{CoreError, CoreResult};
use neo_payloads::{WitnessCondition, WitnessRule};
use neo_primitives::{WitnessRuleAction, strip_hex_prefix};
use neo_serialization::json::{JArray, JObject};

pub fn rule_from_json(
    json: &JObject,
    protocol_settings: &ProtocolSettings,
) -> CoreResult<WitnessRule> {
    let action_str = json
        .get("action")
        .and_then(neo_serialization::json::JToken::as_string)
        .ok_or_else(|| CoreError::other("WitnessRule missing action"))?;
    let action: WitnessRuleAction = action_str
        .parse()
        .map_err(|e: String| CoreError::other(e))?;
    let condition_token = json
        .get("condition")
        .and_then(|value| value.as_object())
        .ok_or_else(|| CoreError::other("WitnessRule missing condition"))?;
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
) -> CoreResult<WitnessCondition> {
    if max_depth == 0 {
        return Err(CoreError::other("Max nesting depth exceeded"));
    }

    let condition_type = json
        .get("type")
        .and_then(neo_serialization::json::JToken::as_string)
        .ok_or_else(|| CoreError::other("Condition type missing"))?;

    match condition_type.as_str() {
        "Or" => {
            let expressions = json
                .get("expressions")
                .and_then(|value| value.as_array())
                .ok_or_else(|| CoreError::other("Or condition missing expressions"))?;
            parse_composite(expressions, protocol_settings, max_depth, true)
        }
        "And" => {
            let expressions = json
                .get("expressions")
                .and_then(|value| value.as_array())
                .ok_or_else(|| CoreError::other("And condition missing expressions"))?;
            parse_composite(expressions, protocol_settings, max_depth, false)
        }
        "Boolean" => {
            let expression = json
                .get("expression")
                .or_else(|| json.get("value"))
                .ok_or_else(|| CoreError::other("Boolean condition missing expression"))?;
            Ok(WitnessCondition::Boolean {
                value: expression.as_boolean(),
            })
        }
        "Not" => {
            let expression = json
                .get("expression")
                .and_then(|value| value.as_object())
                .ok_or_else(|| CoreError::other("Not condition missing expression"))?;
            let condition = condition_from_json(expression, protocol_settings, max_depth - 1)?;
            Ok(WitnessCondition::Not {
                condition: Box::new(condition),
            })
        }
        "Group" => {
            let group = json
                .get("group")
                .and_then(neo_serialization::json::JToken::as_string)
                .ok_or_else(|| CoreError::other("Group condition missing group"))?;
            Ok(WitnessCondition::Group {
                group: parse_group_bytes(&group)?,
            })
        }
        "CalledByContract" => {
            let hash = json
                .get("hash")
                .and_then(neo_serialization::json::JToken::as_string)
                .ok_or_else(|| CoreError::other("CalledByContract missing hash"))?;
            let hash = super::RpcUtility::get_script_hash(&hash, protocol_settings)
                .map_err(|e| CoreError::other(e.to_string()))?;
            Ok(WitnessCondition::CalledByContract { hash })
        }
        "ScriptHash" => {
            let hash = json
                .get("hash")
                .and_then(neo_serialization::json::JToken::as_string)
                .ok_or_else(|| CoreError::other("ScriptHash condition missing hash"))?;
            let hash = super::RpcUtility::get_script_hash(&hash, protocol_settings)
                .map_err(|e| CoreError::other(e.to_string()))?;
            Ok(WitnessCondition::ScriptHash { hash })
        }
        "CalledByEntry" => Ok(WitnessCondition::CalledByEntry),
        "CalledByGroup" => {
            let group = json
                .get("group")
                .and_then(neo_serialization::json::JToken::as_string)
                .ok_or_else(|| CoreError::other("CalledByGroup missing group"))?;
            Ok(WitnessCondition::CalledByGroup {
                group: parse_group_bytes(&group)?,
            })
        }
        other => Err(CoreError::other(format!(
            "Unsupported witness condition type: {other}"
        ))),
    }
}

fn parse_composite(
    expressions: &JArray,
    protocol_settings: &ProtocolSettings,
    max_depth: usize,
    is_or: bool,
) -> CoreResult<WitnessCondition> {
    if expressions.is_empty() {
        return Err(CoreError::other(
            "Composite witness condition requires at least one expression",
        ));
    }
    if expressions.len() > WitnessCondition::MAX_SUBITEMS {
        return Err(CoreError::other(
            "Composite witness condition exceeds max subitems",
        ));
    }

    let mut conditions = Vec::with_capacity(expressions.len());
    for expr in expressions.children() {
        let expr_obj = expr
            .as_ref()
            .and_then(|value| value.as_object())
            .ok_or_else(|| CoreError::other("Witness condition expression must be an object"))?;
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

fn parse_group_bytes(value: &str) -> CoreResult<Vec<u8>> {
    let value = strip_hex_prefix(value);
    let bytes =
        hex::decode(value).map_err(|err| CoreError::other(format!("Invalid hex string: {err}")))?;
    let point = ECPoint::decode(&bytes, ECCurve::secp256r1())
        .map_err(|err| CoreError::other(format!("Invalid ECPoint: {err}")))?;
    point
        .encode_point(true)
        .map_err(|err| CoreError::other(format!("Failed to encode ECPoint: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt160;
    use neo_serialization::json::JToken;
    use neo_wallets::KeyPair;

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
        let uppercase_prefixed_group = format!("0X{}", hex::encode(&group));
        assert_eq!(
            parse_group_bytes(&uppercase_prefixed_group).expect("group"),
            group
        );

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
