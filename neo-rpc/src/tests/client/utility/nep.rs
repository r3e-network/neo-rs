use super::*;

#[test]
fn balance_list_to_json_keeps_balance_before_address() {
    let settings = ProtocolSettings::default_settings();
    let token = balance_list_to_json(&[1_u8], &UInt160::zero(), &settings, |_| {
        let mut item = JObject::new();
        item.insert("value".to_string(), JToken::String("ok".to_string()));
        item
    });

    assert_eq!(
        token.to_string(),
        format!(
            r#"{{"balance":[{{"value":"ok"}}],"address":"{}"}}"#,
            WalletHelper::to_address(&UInt160::zero(), settings.address_version)
        )
    );
}

#[test]
fn nep_balance_fields_preserve_string_amount_and_numeric_height() {
    let amount = BigInt::from(42);
    let mut json = JObject::new();
    insert_nep_balance_fields(
        &mut json,
        NepBalanceFieldRefs {
            amount: &amount,
            last_updated_block: 7,
        },
    );

    assert_eq!(json.to_string(), r#"{"amount":"42","lastupdatedblock":7}"#);

    let parsed = parse_nep_balance_fields(&json).expect("balance fields");
    assert_eq!(parsed.amount, amount);
    assert_eq!(parsed.last_updated_block, 7);
}

#[test]
fn nep_balance_fields_preserve_legacy_type_errors() {
    let mut numeric_amount = JObject::new();
    numeric_amount.insert("amount".to_string(), JToken::Number(1.0));
    numeric_amount.insert("lastupdatedblock".to_string(), JToken::Number(7.0));
    assert_eq!(
        parse_nep_balance_fields(&numeric_amount)
            .expect_err("numeric amount")
            .to_string(),
        "Missing or invalid 'amount' field"
    );

    let mut string_height = JObject::new();
    string_height.insert("amount".to_string(), JToken::String("1".to_string()));
    string_height.insert(
        "lastupdatedblock".to_string(),
        JToken::String("7".to_string()),
    );
    assert_eq!(
        parse_nep_balance_fields(&string_height)
            .expect("string numeric height")
            .last_updated_block,
        7
    );

    let mut invalid_height = JObject::new();
    invalid_height.insert("amount".to_string(), JToken::String("1".to_string()));
    invalid_height.insert(
        "lastupdatedblock".to_string(),
        JToken::String("bad".to_string()),
    );
    assert_eq!(
        parse_nep_balance_fields(&invalid_height)
            .expect_err("invalid height")
            .to_string(),
        "Missing or invalid 'lastupdatedblock' field"
    );

    let mut invalid_amount = JObject::new();
    invalid_amount.insert("amount".to_string(), JToken::String("bad".to_string()));
    invalid_amount.insert("lastupdatedblock".to_string(), JToken::Number(7.0));
    assert_eq!(
        parse_nep_balance_fields(&invalid_amount)
            .expect_err("invalid amount")
            .to_string(),
        "Invalid amount: bad"
    );
}
