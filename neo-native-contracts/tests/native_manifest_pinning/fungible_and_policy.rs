use super::*;

#[test]
fn gas_token_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = GasToken::new();

    // C# GasToken: the five FungibleToken NEP-17 methods, all ungated.
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("balanceOf", &["account"]),
            m("decimals", &[]),
            m("symbol", &[]),
            m("totalSupply", &[]),
            m("transfer", &["from", "to", "amount", "data"]),
        ]
    );

    // FungibleToken.cs:59-62 — the inherited Transfer event, order 0, ungated.
    let transfer = e(
        "Transfer",
        &[("from", Hash160), ("to", Hash160), ("amount", Integer)],
    );
    assert_eq!(
        manifest_events(&contract, &settings, ALL_ACTIVE),
        vec![transfer.clone()]
    );
    assert_eq!(
        manifest_events(&contract, &settings, GENESIS),
        vec![transfer]
    );
}

#[test]
fn neo_token_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = NeoToken::new();

    // C# NeoToken reflection names (NeoToken.cs + the FungibleToken base).
    // Note getCandidateVote's capital-K `pubKey` vs registerCandidate's
    // `pubkey` — both verbatim C# parameter spellings. onNEP17Payment is the
    // Echidna-gated candidate-registration-by-GAS-payment callback
    // (NeoToken.cs:374).
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("balanceOf", &["account"]),
            m("decimals", &[]),
            m("getAccountState", &["account"]),
            m("getAllCandidates", &[]),
            m("getCandidateVote", &["pubKey"]),
            m("getCandidates", &[]),
            m("getCommittee", &[]),
            m("getCommitteeAddress", &[]),
            m("getGasPerBlock", &[]),
            m("getNextBlockValidators", &[]),
            m("getRegisterPrice", &[]),
            m("onNEP17Payment", &["from", "amount", "data"]),
            m("registerCandidate", &["pubkey"]),
            m("setGasPerBlock", &["gasPerBlock"]),
            m("setRegisterPrice", &["registerPrice"]),
            m("symbol", &[]),
            m("totalSupply", &[]),
            m("transfer", &["from", "to", "amount", "data"]),
            m("unclaimedGas", &["account", "end"]),
            m("unregisterCandidate", &["pubkey"]),
            m("vote", &["account", "voteTo"]),
        ]
    );

    // onNEP17Payment is `[ContractMethod(Hardfork.HF_Echidna, …)]`
    // (NeoToken.cs:374): absent just below the boundary, present at it.
    assert!(
        !manifest_methods(&contract, &settings, 49)
            .iter()
            .any(|(name, _)| name == "onNEP17Payment")
    );
    assert!(
        manifest_methods(&contract, &settings, 50)
            .iter()
            .any(|(name, _)| name == "onNEP17Payment")
    );
    // getAllCandidates is ungated (genesis-active).
    assert!(
        manifest_methods(&contract, &settings, GENESIS)
            .iter()
            .any(|(name, _)| name == "getAllCandidates")
    );

    // NeoToken.cs:63-74 + the inherited Transfer: orders 0..3, with
    // CommitteeChanged ActiveIn HF_Cockatrice.
    let transfer = e(
        "Transfer",
        &[("from", Hash160), ("to", Hash160), ("amount", Integer)],
    );
    let candidate_state_changed = e(
        "CandidateStateChanged",
        &[
            ("pubkey", PublicKey),
            ("registered", Boolean),
            ("votes", Integer),
        ],
    );
    let vote = e(
        "Vote",
        &[
            ("account", Hash160),
            ("from", PublicKey),
            ("to", PublicKey),
            ("amount", Integer),
        ],
    );
    let committee_changed = e("CommitteeChanged", &[("old", Array), ("new", Array)]);

    assert_eq!(
        manifest_events(&contract, &settings, ALL_ACTIVE),
        vec![
            transfer.clone(),
            candidate_state_changed.clone(),
            vote.clone(),
            committee_changed.clone(),
        ]
    );
    // Pre-Cockatrice (height 29): CommitteeChanged is absent.
    assert_eq!(
        manifest_events(&contract, &settings, 29),
        vec![transfer, candidate_state_changed, vote]
    );
    // At the Cockatrice boundary (height 30) it appears.
    assert_eq!(
        manifest_events(&contract, &settings, 30).last(),
        Some(&committee_changed)
    );
}

#[test]
fn policy_contract_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = PolicyContract::new();

    // C# PolicyContract reflection names at a height where Echidna and Faun
    // are active (blockAccount = the Faun V1 registration; the deprecated V0
    // has dropped out of the manifest).
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("blockAccount", &["account"]),
            m("getAttributeFee", &["attributeType"]),
            m("getBlockedAccounts", &[]),
            m("getExecFeeFactor", &[]),
            m("getExecPicoFeeFactor", &[]),
            m("getFeePerByte", &[]),
            m("getMaxTraceableBlocks", &[]),
            m("getMaxTransactionsPerBlock", &[]),
            m("getMaxValidUntilBlockIncrement", &[]),
            m("getMillisecondsPerBlock", &[]),
            m("getStoragePrice", &[]),
            m("getWhitelistFeeContracts", &[]),
            m("isBlocked", &["account"]),
            m("recoverFund", &["account", "token"]),
            m(
                "removeWhitelistFeeContract",
                &["contractHash", "method", "argCount"]
            ),
            m("setAttributeFee", &["attributeType", "value"]),
            m("setExecFeeFactor", &["value"]),
            m("setFeePerByte", &["value"]),
            m("setMaxTraceableBlocks", &["value"]),
            m("setMaxTransactionsPerBlock", &["value"]),
            m("setMaxValidUntilBlockIncrement", &["value"]),
            m("setMillisecondsPerBlock", &["value"]),
            m("setStoragePrice", &["value"]),
            m(
                "setWhitelistFeeContract",
                &["contractHash", "method", "argCount", "fixedFee"]
            ),
            m("unblockAccount", &["account"]),
        ]
    );

    // PolicyContract.cs:115-125: all three events hardfork-gated.
    let ms_per_block_changed = e(
        "MillisecondsPerBlockChanged",
        &[("old", Integer), ("new", Integer)],
    );
    let whitelist_fee_changed = e(
        "WhitelistFeeChanged",
        &[
            ("contract", Hash160),
            ("method", StringT),
            ("argCount", Integer),
            ("fee", Any),
        ],
    );
    let recovered_fund = e("RecoveredFund", &[("account", Hash160)]);

    // Genesis: no events at all.
    assert_eq!(manifest_events(&contract, &settings, GENESIS), vec![]);
    // Echidna active, Faun not (height 50..59): only MillisecondsPerBlockChanged.
    assert_eq!(
        manifest_events(&contract, &settings, 50),
        vec![ms_per_block_changed.clone()]
    );
    // Faun active: all three, in attribute order 0,1,2.
    assert_eq!(
        manifest_events(&contract, &settings, ALL_ACTIVE),
        vec![ms_per_block_changed, whitelist_fee_changed, recovered_fund]
    );
}
