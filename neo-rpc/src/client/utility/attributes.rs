use super::parsing::{
    parse_base64_token, parse_oracle_response_code, parse_u32_token, parse_u64_token,
};
use neo_core::TransactionAttribute;
use neo_core::network::p2p::payloads::{
    conflicts::Conflicts, not_valid_before::NotValidBefore, notary_assisted::NotaryAssisted,
    oracle_response::OracleResponse,
};
use neo_json::JObject;
use neo_primitives::UInt256;

/// Parses a transaction attribute from RPC JSON.
pub fn attribute_from_json(json: &JObject) -> Result<TransactionAttribute, String> {
    let attr_type = json
        .get("type")
        .and_then(neo_json::JToken::as_string)
        .ok_or("Transaction attribute missing 'type' field")?;

    match attr_type.as_str() {
        "HighPriority" => Ok(TransactionAttribute::HighPriority),
        "NotValidBefore" => {
            let height_token = json
                .get("height")
                .ok_or("NotValidBefore attribute missing 'height' field")?;
            let height = parse_u32_token(height_token, "height")?;
            Ok(TransactionAttribute::NotValidBefore(NotValidBefore::new(
                height,
            )))
        }
        "Conflicts" => {
            let hash_str = json
                .get("hash")
                .and_then(neo_json::JToken::as_string)
                .ok_or("Conflicts attribute missing 'hash' field")?;
            let hash = UInt256::parse(&hash_str)
                .map_err(|err| format!("Invalid conflicts hash: {err}"))?;
            Ok(TransactionAttribute::Conflicts(Conflicts::new(hash)))
        }
        "NotaryAssisted" => {
            let nkeys_token = json
                .get("nkeys")
                .ok_or("NotaryAssisted attribute missing 'nkeys' field")?;
            let nkeys = parse_u32_token(nkeys_token, "nkeys")?;
            Ok(TransactionAttribute::NotaryAssisted(NotaryAssisted::new(
                nkeys as u8,
            )))
        }
        "OracleResponse" => {
            let id_token = json
                .get("id")
                .ok_or("OracleResponse attribute missing 'id' field")?;
            let id = parse_u64_token(id_token, "id")?;
            let code_token = json
                .get("code")
                .ok_or("OracleResponse attribute missing 'code' field")?;
            let code = parse_oracle_response_code(code_token)?;
            let result_token = json
                .get("result")
                .ok_or("OracleResponse attribute missing 'result' field")?;
            let result = parse_base64_token(result_token, "result")?;
            Ok(TransactionAttribute::OracleResponse(OracleResponse::new(
                id, code, result,
            )))
        }
        other => Err(format!(
            "Unsupported transaction attribute type '{other}' in RPC payload"
        )),
    }
}
