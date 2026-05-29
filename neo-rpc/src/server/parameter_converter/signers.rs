use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use hex;
use neo_crypto::{ECCurve, ECPoint};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::MAX_TRANSACTION_ATTRIBUTES;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::{WitnessRule, WitnessScope};
use neo_json::JToken;

use super::super::model::SignersAndWitnesses;
use super::super::rpc_exception::RpcException;
use super::{
    expect_array, expect_object, expect_string, invalid_params, jtoken_to_serde, parse_address,
    parse_uint160, ConversionContext,
};

pub(super) fn parse_signers_and_witnesses(
    token: &JToken,
    ctx: &ConversionContext,
) -> Result<SignersAndWitnesses, RpcException> {
    let array = expect_array(token)?;
    if array.count() > MAX_TRANSACTION_ATTRIBUTES {
        return Err(invalid_params("Max allowed signers exceeded"));
    }

    let mut signers = Vec::with_capacity(array.count());
    let mut witnesses = Vec::new();

    for (index, entry) in array.children().iter().enumerate() {
        let token = entry
            .as_ref()
            .ok_or_else(|| invalid_params(format!("Invalid signer entry at index {index}")))?;
        let obj = expect_object(token)?;

        let signer_token = obj.get("signer").unwrap_or(token);
        let signer = parse_signer(signer_token, ctx)?;
        signers.push(signer);

        if let Some(witness_token) = obj.get("witness") {
            if !matches!(witness_token, JToken::Null) {
                let witness = parse_witness(witness_token)?;
                witnesses.push(witness);
            }
        } else if obj.contains_property("invocation") || obj.contains_property("verification") {
            let witness = parse_witness(token)?;
            witnesses.push(witness);
        }
    }

    Ok(SignersAndWitnesses::new(signers, witnesses))
}

fn parse_signer(token: &JToken, ctx: &ConversionContext) -> Result<Signer, RpcException> {
    let obj = expect_object(token)?;

    let account_token = obj
        .get("account")
        .ok_or_else(|| invalid_params("Signer is missing 'account' field"))?;
    let account_text = expect_string(account_token, "Signer 'account' must be a string")?;
    let account = *parse_address(&account_text, ctx.address_version)?.script_hash();

    let scopes_token = obj
        .get("scopes")
        .ok_or_else(|| invalid_params("Signer is missing 'scopes' field"))?;
    let scopes_text = expect_string(scopes_token, "Signer 'scopes' must be a string")?;
    let scopes = parse_witness_scope(&scopes_text)?;

    let mut signer = Signer {
        account,
        scopes,
        allowed_contracts: Vec::new(),
        allowed_groups: Vec::new(),
        rules: Vec::new(),
    };

    if scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
        if let Some(contracts_token) = obj.get("allowedcontracts") {
            let array = expect_array(contracts_token)?;
            signer.allowed_contracts = array
                .children()
                .iter()
                .map(|item| {
                    let contract = item
                        .as_ref()
                        .ok_or_else(|| invalid_params("Null contract entry"))?;
                    let text =
                        expect_string(contract, "Allowed contract entries must be strings")?;
                    parse_uint160(&text)
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }

    if scopes.contains(WitnessScope::CUSTOM_GROUPS) {
        if let Some(groups_token) = obj.get("allowedgroups") {
            let array = expect_array(groups_token)?;
            signer.allowed_groups = array
                .children()
                .iter()
                .map(|item| {
                    let group = item
                        .as_ref()
                        .ok_or_else(|| invalid_params("Null group entry"))?;
                    let text = expect_string(group, "Allowed group entries must be strings")?;
                    let bytes = hex::decode(text.trim_start_matches("0x"))
                        .map_err(|_| invalid_params("Invalid ECPoint"))?;
                    ECPoint::new(ECCurve::Secp256r1, bytes)
                        .map_err(|e| invalid_params(format!("Invalid ECPoint: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }

    if scopes.contains(WitnessScope::WITNESS_RULES) {
        if let Some(rules_token) = obj.get("rules") {
            let array = expect_array(rules_token)?;
            signer.rules = array
                .children()
                .iter()
                .map(|item| {
                    let value = item
                        .as_ref()
                        .ok_or_else(|| invalid_params("Null witness rule"))?;
                    let json = jtoken_to_serde(value);
                    WitnessRule::from_json(&json)
                        .map_err(|e| invalid_params(format!("Invalid witness rule: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
    }

    Ok(signer)
}

fn parse_witness(token: &JToken) -> Result<Witness, RpcException> {
    let obj = expect_object(token)?;
    let invocation = obj
        .get("invocation")
        .map(|t| {
            let text = expect_string(t, "Invocation script must be a string")?;
            BASE64_STANDARD
                .decode(text.trim())
                .map_err(|_| invalid_params("Invalid invocation script"))
        })
        .transpose()?
        .unwrap_or_default();

    let verification = obj
        .get("verification")
        .map(|t| {
            let text = expect_string(t, "Verification script must be a string")?;
            BASE64_STANDARD
                .decode(text.trim())
                .map_err(|_| invalid_params("Invalid verification script"))
        })
        .transpose()?
        .unwrap_or_default();

    Ok(Witness::new_with_scripts(invocation, verification))
}

pub(super) fn parse_witness_scope(text: &str) -> Result<WitnessScope, RpcException> {
    let cleaned = text.replace(' ', "");
    if cleaned.is_empty() {
        return Ok(WitnessScope::NONE);
    }

    let mut value: u8 = 0;
    for part in cleaned.split(['|', ',']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let flag = match part {
            "None" => 0x00,
            "CalledByEntry" => WitnessScope::CALLED_BY_ENTRY.bits(),
            "CustomContracts" => WitnessScope::CUSTOM_CONTRACTS.bits(),
            "CustomGroups" => WitnessScope::CUSTOM_GROUPS.bits(),
            "WitnessRules" => WitnessScope::WITNESS_RULES.bits(),
            "Global" => WitnessScope::GLOBAL.bits(),
            other => return Err(invalid_params(format!("Unknown witness scope: {other}"))),
        };

        if flag == WitnessScope::GLOBAL.bits() && value != 0 {
            return Err(invalid_params(
                "Global scope cannot be combined with other scopes",
            ));
        }
        value |= flag;
    }

    WitnessScope::from_byte(value)
        .ok_or_else(|| invalid_params(format!("Invalid witness scope combination: {text}")))
}
