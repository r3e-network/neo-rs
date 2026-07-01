use super::*;

#[test]
fn contract_management_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = ContractManagement::new();

    // C# ContractManagement reflection names; deploy/update are dual-arity
    // overloads ordered by parameter count.
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("deploy", &["nefFile", "manifest"]),
            m("deploy", &["nefFile", "manifest", "data"]),
            m("destroy", &[]),
            m("getContract", &["hash"]),
            m("getContractById", &["id"]),
            m("getContractHashes", &[]),
            m("getMinimumDeploymentFee", &[]),
            m("hasMethod", &["hash", "method", "pcount"]),
            m("isContract", &["hash"]),
            m("setMinimumDeploymentFee", &["value"]),
            m("update", &["nefFile", "manifest"]),
            m("update", &["nefFile", "manifest", "data"]),
        ]
    );

    // ContractManagement.cs:40-42 — three ungated events with a capital-H
    // `Hash` parameter, in attribute order Deploy, Update, Destroy.
    let expected = vec![
        e("Deploy", &[("Hash", Hash160)]),
        e("Update", &[("Hash", Hash160)]),
        e("Destroy", &[("Hash", Hash160)]),
    ];
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), expected);
    assert_eq!(manifest_events(&contract, &settings, GENESIS), expected);
}

#[test]
fn oracle_contract_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = OracleContract::new();

    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("finish", &[]),
            m("getPrice", &[]),
            m(
                "request",
                &["url", "filter", "callback", "userData", "gasForResponse"]
            ),
            m("setPrice", &["price"]),
            m("verify", &[]),
        ]
    );

    // OracleContract.cs:46-53 — both ungated, orders 0 and 1; the attribute
    // arguments are capitalized (Id, RequestContract, Url, Filter, OriginalTx).
    let expected = vec![
        e(
            "OracleRequest",
            &[
                ("Id", Integer),
                ("RequestContract", Hash160),
                ("Url", StringT),
                ("Filter", StringT),
            ],
        ),
        e(
            "OracleResponse",
            &[("Id", Integer), ("OriginalTx", Hash256)],
        ),
    ];
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), expected);
    assert_eq!(manifest_events(&contract, &settings, GENESIS), expected);
}

#[test]
fn role_management_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = RoleManagement::new();

    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("designateAsRole", &["role", "nodes"]),
            m("getDesignatedByRole", &["role", "index"]),
        ]
    );

    // RoleManagement.cs:27-37 — the DUAL Designation registration, both at
    // order 0: V0 (Role, BlockIndex) is DeprecatedIn HF_Echidna; V1 adds
    // (Old, New) and is ActiveIn HF_Echidna. Exactly one per height.
    let v0 = e("Designation", &[("Role", Integer), ("BlockIndex", Integer)]);
    let v1 = e(
        "Designation",
        &[
            ("Role", Integer),
            ("BlockIndex", Integer),
            ("Old", Array),
            ("New", Array),
        ],
    );
    assert_eq!(
        manifest_events(&contract, &settings, GENESIS),
        vec![v0.clone()]
    );
    // Just below the Echidna boundary (height 49): still V0.
    assert_eq!(manifest_events(&contract, &settings, 49), vec![v0]);
    // At and beyond Echidna (height 50): V1 replaces it.
    assert_eq!(manifest_events(&contract, &settings, 50), vec![v1.clone()]);
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![v1]);

    // The Echidna boundary must be an initialize block for RoleManagement —
    // in C# the event attributes put HF_Echidna into _usedHardforks, which is
    // the only thing that refreshes this manifest at the boundary.
    let (refresh, hits) = contract.is_initialize_block(&settings, 50);
    assert!(refresh);
    assert_eq!(hits, vec![Hardfork::HfEchidna]);
}

#[test]
fn notary_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = Notary::new();

    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("balanceOf", &["account"]),
            m("expirationOf", &["account"]),
            m("getMaxNotValidBeforeDelta", &[]),
            m("lockDepositUntil", &["account", "till"]),
            m("onNEP17Payment", &["from", "amount", "data"]),
            m("setMaxNotValidBeforeDelta", &["value"]),
            m("verify", &["signature"]),
            m("withdraw", &["from", "to"]),
        ]
    );

    // Notary declares no [ContractEvent] in C# v3.10.0.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}

#[test]
fn ledger_contract_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = LedgerContract::new();

    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("currentHash", &[]),
            m("currentIndex", &[]),
            m("getBlock", &["indexOrHash"]),
            m("getTransaction", &["hash"]),
            m("getTransactionFromBlock", &["blockIndexOrHash", "txIndex"]),
            m("getTransactionHeight", &["hash"]),
            m("getTransactionSigners", &["hash"]),
            m("getTransactionVMState", &["hash"]),
        ]
    );

    // LedgerContract declares no [ContractEvent] in C# v3.10.0.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}

#[test]
fn std_lib_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = StdLib::new();

    // C# StdLib reflection names; `itoa`/`atoi`'s second C# parameter is
    // `int @base` whose reflection name is "base". Ordinal name sort puts
    // strLen before stringSplit ('L' < 'i').
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("atoi", &["value"]),
            m("atoi", &["value", "base"]),
            m("base58CheckDecode", &["s"]),
            m("base58CheckEncode", &["data"]),
            m("base58Decode", &["s"]),
            m("base58Encode", &["data"]),
            m("base64Decode", &["s"]),
            m("base64Encode", &["data"]),
            m("base64UrlDecode", &["s"]),
            m("base64UrlEncode", &["data"]),
            m("deserialize", &["data"]),
            m("hexDecode", &["str"]),
            m("hexEncode", &["bytes"]),
            m("itoa", &["value"]),
            m("itoa", &["value", "base"]),
            m("jsonDeserialize", &["json"]),
            m("jsonSerialize", &["item"]),
            m("memoryCompare", &["str1", "str2"]),
            m("memorySearch", &["mem", "value"]),
            m("memorySearch", &["mem", "value", "start"]),
            m("memorySearch", &["mem", "value", "start", "backward"]),
            m("serialize", &["item"]),
            m("strLen", &["str"]),
            m("stringSplit", &["str", "separator"]),
            m("stringSplit", &["str", "separator", "removeEmptyEntries"]),
        ]
    );

    // StdLib declares no [ContractEvent] in C# v3.10.0.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}

#[test]
fn crypto_lib_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = CryptoLib::new();

    // All hardforks active: verifyWithECDsa is the Gorgon V2 descriptor and
    // verifyWithEd25519 is the Gorgon V1 descriptor. Their ABI names match
    // the earlier registrations, but the native method cache must select the
    // hardfork-specific implementation metadata.
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("bls12381Add", &["x", "y"]),
            m("bls12381Deserialize", &["data"]),
            m("bls12381Equal", &["x", "y"]),
            m("bls12381Mul", &["x", "mul", "neg"]),
            m("bls12381Pairing", &["g1", "g2"]),
            m("bls12381Serialize", &["g"]),
            m("keccak256", &["data"]),
            m("murmur32", &["data", "seed"]),
            m("recoverSecp256K1", &["messageHash", "signature"]),
            m("ripemd160", &["data"]),
            m("sha256", &["data"]),
            m(
                "verifyWithECDsa",
                &["message", "pubkey", "signature", "curveHash"]
            ),
            m("verifyWithEd25519", &["message", "pubkey", "signature"]),
        ]
    );

    // Genesis: the Cockatrice/Echidna methods are gone and verifyWithECDsa
    // is the genesis V0 whose fourth C# parameter is `curve`.
    assert_eq!(
        manifest_methods(&contract, &settings, GENESIS),
        vec![
            m("bls12381Add", &["x", "y"]),
            m("bls12381Deserialize", &["data"]),
            m("bls12381Equal", &["x", "y"]),
            m("bls12381Mul", &["x", "mul", "neg"]),
            m("bls12381Pairing", &["g1", "g2"]),
            m("bls12381Serialize", &["g"]),
            m("murmur32", &["data", "seed"]),
            m("ripemd160", &["data"]),
            m("sha256", &["data"]),
            m(
                "verifyWithECDsa",
                &["message", "pubkey", "signature", "curve"]
            ),
        ]
    );

    // Cockatrice active, Gorgon inactive: verifyWithECDsa is V1
    // (ActiveIn Cockatrice, DeprecatedIn Gorgon), while Ed25519 is not active
    // until Echidna.
    assert_eq!(
        manifest_methods(&contract, &settings, 30),
        vec![
            m("bls12381Add", &["x", "y"]),
            m("bls12381Deserialize", &["data"]),
            m("bls12381Equal", &["x", "y"]),
            m("bls12381Mul", &["x", "mul", "neg"]),
            m("bls12381Pairing", &["g1", "g2"]),
            m("bls12381Serialize", &["g"]),
            m("keccak256", &["data"]),
            m("murmur32", &["data", "seed"]),
            m("ripemd160", &["data"]),
            m("sha256", &["data"]),
            m(
                "verifyWithECDsa",
                &["message", "pubkey", "signature", "curveHash"]
            ),
        ]
    );

    // Echidna active, Gorgon inactive: Ed25519 V0 joins and ECDSA remains V1.
    assert_eq!(
        manifest_methods(&contract, &settings, 50),
        vec![
            m("bls12381Add", &["x", "y"]),
            m("bls12381Deserialize", &["data"]),
            m("bls12381Equal", &["x", "y"]),
            m("bls12381Mul", &["x", "mul", "neg"]),
            m("bls12381Pairing", &["g1", "g2"]),
            m("bls12381Serialize", &["g"]),
            m("keccak256", &["data"]),
            m("murmur32", &["data", "seed"]),
            m("recoverSecp256K1", &["messageHash", "signature"]),
            m("ripemd160", &["data"]),
            m("sha256", &["data"]),
            m(
                "verifyWithECDsa",
                &["message", "pubkey", "signature", "curveHash"]
            ),
            m("verifyWithEd25519", &["message", "pubkey", "signature"]),
        ]
    );

    // At Gorgon, the V2/V1 methods replace the older V1/V0 registrations.
    assert_eq!(
        manifest_methods(&contract, &settings, 70),
        manifest_methods(&contract, &settings, ALL_ACTIVE)
    );

    // CryptoLib declares no [ContractEvent] in C# v3.10.0.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}

#[test]
fn treasury_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = Treasury::new();

    // C# Treasury reflection names (Treasury.cs:41-63): the committee-witness
    // verify plus the two no-op payment callbacks.
    assert_eq!(
        manifest_methods(&contract, &settings, ALL_ACTIVE),
        vec![
            m("onNEP11Payment", &["from", "amount", "tokenId", "data"]),
            m("onNEP17Payment", &["from", "amount", "data"]),
            m("verify", &[]),
        ]
    );

    // Treasury declares no [ContractEvent] in C# v3.10.0.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}
