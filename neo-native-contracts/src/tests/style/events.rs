use super::standard_contract_sources;

#[test]
fn role_management_designation_event_uses_shared_event_name() {
    let source = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "RoleManagement")
        .map(|(_, source)| source)
        .expect("RoleManagement source should be available");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);

    assert!(
        production.contains("ROLE_DESIGNATION_EVENT"),
        "RoleManagement should share one constant for the Designation event name"
    );
    assert!(
        !production.contains("\"Designation\".to_string()"),
        "RoleManagement runtime notifications should not duplicate the Designation event string"
    );
    assert!(
        !production.contains("NativeEvent::new(\n            0,\n            \"Designation\""),
        "RoleManagement manifest events should not duplicate the Designation event string"
    );
}

#[test]
fn contract_management_events_use_shared_event_names() {
    let source = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "ContractManagement")
        .map(|(_, source)| source)
        .expect("ContractManagement source should be available");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);

    for event_const in [
        "CONTRACT_DEPLOY_EVENT",
        "CONTRACT_UPDATE_EVENT",
        "CONTRACT_DESTROY_EVENT",
    ] {
        assert!(
            production.contains(event_const),
            "ContractManagement should share {event_const} between metadata and runtime notifications"
        );
    }

    for event_name in ["Deploy", "Update", "Destroy"] {
        assert!(
            !production.contains(&format!("NativeEvent::new(0, \"{event_name}\""))
                && !production.contains(&format!("NativeEvent::new(1, \"{event_name}\""))
                && !production.contains(&format!("NativeEvent::new(2, \"{event_name}\"")),
            "ContractManagement manifest events should not duplicate the {event_name} event string"
        );
        assert!(
            !production.contains(&format!("\"{event_name}\".to_string()")),
            "ContractManagement runtime notifications should not duplicate the {event_name} event string"
        );
    }

    assert!(
        !production.contains("if is_create { \"Deploy\" } else { \"Update\" }.to_string()"),
        "ContractManagement on_persist should select Deploy/Update through shared event constants"
    );
    assert!(
        !production.contains("if update { \"Update\" } else { \"Deploy\" }"),
        "ContractManagement on_deploy should select Deploy/Update through shared event constants"
    );
}

#[test]
fn neo_token_events_use_shared_event_names() {
    let source = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "NeoToken")
        .map(|(_, source)| source)
        .expect("NeoToken source should be available");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);

    for event_const in [
        "NEO_CANDIDATE_STATE_CHANGED_EVENT",
        "NEO_VOTE_EVENT",
        "NEO_COMMITTEE_CHANGED_EVENT",
    ] {
        assert!(
            production.contains(event_const),
            "NeoToken should share {event_const} between metadata and runtime notifications"
        );
    }

    for event_name in ["CandidateStateChanged", "Vote", "CommitteeChanged"] {
        assert!(
            !production.contains(&format!("\"{event_name}\".to_string()")),
            "NeoToken runtime notifications should not duplicate the {event_name} event string"
        );
        assert!(
            !production.contains(&format!(
                "NativeEvent::new(\n            1,\n            \"{event_name}\""
            )) && !production.contains(&format!(
                "NativeEvent::new(\n            2,\n            \"{event_name}\""
            )) && !production.contains(&format!(
                "NativeEvent::new(\n            3,\n            \"{event_name}\""
            )),
            "NeoToken manifest events should not duplicate the {event_name} event string"
        );
    }
}

#[test]
fn policy_contract_events_use_shared_event_names() {
    let policy_source = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "PolicyContract")
        .map(|(_, source)| source)
        .expect("PolicyContract source should be available");
    let policy_production = policy_source
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(policy_source);

    for event_const in [
        "POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT",
        "POLICY_WHITELIST_FEE_CHANGED_EVENT",
        "POLICY_RECOVERED_FUND_EVENT",
    ] {
        assert!(
            policy_production.contains(event_const),
            "PolicyContract should share {event_const} between metadata and runtime notifications"
        );
    }

    for (event_name, order) in [
        ("MillisecondsPerBlockChanged", 0),
        ("WhitelistFeeChanged", 1),
        ("RecoveredFund", 2),
    ] {
        assert!(
            !policy_production.contains(&format!("\"{event_name}\".to_string()")),
            "PolicyContract runtime notifications should not duplicate the {event_name} event string"
        );
        assert!(
            !policy_production.contains(&format!(
                "NativeEvent::new(\n            {order},\n            \"{event_name}\""
            )),
            "PolicyContract manifest events should not duplicate the {event_name} event string"
        );
    }

    let contract_management_source = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "ContractManagement")
        .map(|(_, source)| source)
        .expect("ContractManagement source should be available");
    let contract_management_production = contract_management_source
        .split("#[cfg(test)]")
        .next()
        .unwrap_or(contract_management_source);

    assert!(
        contract_management_production.contains("POLICY_WHITELIST_FEE_CHANGED_EVENT"),
        "ContractManagement should reuse the PolicyContract WhitelistFeeChanged event name"
    );
    assert!(
        !contract_management_production.contains("\"WhitelistFeeChanged\".to_string()"),
        "ContractManagement should not duplicate PolicyContract's WhitelistFeeChanged event string"
    );
}

#[test]
fn oracle_contract_events_use_shared_event_names() {
    let source = standard_contract_sources()
        .into_iter()
        .find(|(name, _)| *name == "OracleContract")
        .map(|(_, source)| source)
        .expect("OracleContract source should be available");
    let production = source.split("#[cfg(test)]").next().unwrap_or(source);

    for event_const in ["ORACLE_REQUEST_EVENT", "ORACLE_RESPONSE_EVENT"] {
        assert!(
            production.contains(event_const),
            "OracleContract should share {event_const} between metadata and runtime notifications"
        );
    }

    for (event_name, order) in [("OracleRequest", 0), ("OracleResponse", 1)] {
        assert!(
            !production.contains(&format!("\"{event_name}\".to_string()")),
            "OracleContract runtime notifications should not duplicate the {event_name} event string"
        );
        assert!(
            !production.contains(&format!(
                "NativeEvent::new(\n            {order},\n            \"{event_name}\""
            )),
            "OracleContract manifest events should not duplicate the {event_name} event string"
        );
    }
}
