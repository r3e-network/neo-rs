//! Native-manifest metadata pinning: method parameter NAMES, per-method
//! `safe` flags, and event lists.
//!
//! One test per native contract asserting the COMPOSED manifest ABI (via
//! `build_native_contract_state`, the same path that produces the stored,
//! consensus-observable native contract state) against hand-written
//! expectations derived from the C# v3.9.1 sources — the `[ContractMethod]`
//! reflection parameter names and the `[ContractEvent]` constructor
//! attributes — NOT from the Rust method/event tables the implementation
//! itself uses.
//!
//! C# reference points (neo_csharp/src/Neo/SmartContract/Native):
//! - `ContractMethodMetadata.cs`: manifest parameter names = the C# method's
//!   reflection parameter names after the leading engine/snapshot parameter;
//!   manifest methods are ordered `OrderBy(Name, Ordinal).ThenBy(Parameters.Length)`.
//! - `ContractMethodMetadata.cs:74`: the manifest Safe flag is DERIVED, never
//!   hand-set: `Safe = (attribute.RequiredCallFlags & ~CallFlags.ReadOnly) == 0`
//!   with `ReadOnly = ReadStates | AllowCall` (CallFlags.cs:55) — i.e. a
//!   method is safe iff it needs neither WriteStates nor AllowNotify.
//! - `NativeContract.cs` (GetContractState): events =
//!   `_eventsDescriptors.Where(IsActive).Select(Descriptor)` with the
//!   declarations pre-sorted by the attribute's `order` argument.
//! - Each contract's `[ContractEvent]` attributes for names/params/gating.

use std::collections::HashMap;

use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::native_contract::{NativeContract, build_native_contract_state};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{
    ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
    PolicyContract, RoleManagement, StandardNativeProvider, StdLib, Treasury,
    standard_native_contract_specs,
};
use neo_primitives::{CallFlags, ContractParameterType};

/// Test settings scheduling every hardfork at a distinct height so each
/// gating boundary can be probed: Aspidochelone=10, Basilisk=20,
/// Cockatrice=30, Domovoi=40, Echidna=50, Faun=60, Gorgon=70.
fn test_settings() -> ProtocolSettings {
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfAspidochelone, 10);
    hardforks.insert(Hardfork::HfBasilisk, 20);
    hardforks.insert(Hardfork::HfCockatrice, 30);
    hardforks.insert(Hardfork::HfDomovoi, 40);
    hardforks.insert(Hardfork::HfEchidna, 50);
    hardforks.insert(Hardfork::HfFaun, 60);
    hardforks.insert(Hardfork::HfGorgon, 70);
    ProtocolSettings {
        hardforks,
        ..ProtocolSettings::mainnet()
    }
}

/// A height at which every hardfork in `test_settings` is active.
const ALL_ACTIVE: u32 = 100;
/// Genesis: no hardfork active.
const GENESIS: u32 = 0;

/// The composed manifest's methods as `(name, [parameter names])`, in
/// manifest order (sorted by name then parameter count, like C#).
fn manifest_methods(
    contract: &dyn NativeContract,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, Vec<String>)> {
    build_native_contract_state(contract, settings, height)
        .manifest
        .abi
        .methods
        .iter()
        .map(|method| {
            (
                method.name.clone(),
                method
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect(),
            )
        })
        .collect()
}

/// The composed manifest's events as `(name, [(param name, param type)])`,
/// in manifest order (the C# attribute order index).
fn manifest_events(
    contract: &dyn NativeContract,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, Vec<(String, ContractParameterType)>)> {
    build_native_contract_state(contract, settings, height)
        .manifest
        .abi
        .events
        .iter()
        .map(|event| {
            (
                event.name.clone(),
                event
                    .parameters
                    .iter()
                    .map(|parameter| (parameter.name.clone(), parameter.param_type))
                    .collect(),
            )
        })
        .collect()
}

/// The composed manifest's SAFE methods as `(name, parameter count)`, in
/// manifest order — the consensus-observable projection of each method's
/// `safe` flag (parameter count disambiguates same-name overloads like
/// `deploy`/`memorySearch`). The complement of this set against the full
/// method lists pinned per contract above is the not-safe set, so this pins
/// every method's safe flag.
fn manifest_safe_methods(
    contract: &dyn NativeContract,
    settings: &ProtocolSettings,
    height: u32,
) -> Vec<(String, usize)> {
    build_native_contract_state(contract, settings, height)
        .manifest
        .abi
        .methods
        .iter()
        .filter(|method| method.safe)
        .map(|method| (method.name.clone(), method.parameters.len()))
        .collect()
}

/// Expectation literal: a manifest method entry.
fn m(name: &str, params: &[&str]) -> (String, Vec<String>) {
    (
        name.to_string(),
        params.iter().map(|p| (*p).to_string()).collect(),
    )
}

/// Expectation literal: a safe-method entry as `(name, parameter count)`.
fn s(name: &str, arity: usize) -> (String, usize) {
    (name.to_string(), arity)
}

/// Expectation literal: a manifest event entry.
fn e(
    name: &str,
    params: &[(&str, ContractParameterType)],
) -> (String, Vec<(String, ContractParameterType)>) {
    (
        name.to_string(),
        params.iter().map(|(p, t)| ((*p).to_string(), *t)).collect(),
    )
}

use ContractParameterType::{
    Any, Array, Boolean, Hash160, Hash256, Integer, PublicKey, String as StringT,
};

#[test]
fn native_contract_handle_names_are_uniform_constants() {
    let contracts: Vec<(&'static str, Box<dyn NativeContract>)> = vec![
        (
            ContractManagement::NAME,
            Box::new(ContractManagement::new()),
        ),
        (StdLib::NAME, Box::new(StdLib::new())),
        (CryptoLib::NAME, Box::new(CryptoLib::new())),
        (LedgerContract::NAME, Box::new(LedgerContract::new())),
        (NeoToken::NAME, Box::new(NeoToken::new())),
        (GasToken::NAME, Box::new(GasToken::new())),
        (PolicyContract::NAME, Box::new(PolicyContract::new())),
        (RoleManagement::NAME, Box::new(RoleManagement::new())),
        (OracleContract::NAME, Box::new(OracleContract::new())),
        (Notary::NAME, Box::new(Notary::new())),
        (Treasury::NAME, Box::new(Treasury::new())),
    ];

    assert_eq!(
        contracts.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        vec![
            "ContractManagement",
            "StdLib",
            "CryptoLib",
            "LedgerContract",
            "NeoToken",
            "GasToken",
            "PolicyContract",
            "RoleManagement",
            "OracleContract",
            "Notary",
            "Treasury",
        ]
    );

    for (name, contract) in contracts {
        assert_eq!(contract.name(), name);
    }
}

mod native_handle_api {
    use neo_native_contracts::{
        ContractManagement, CryptoLib, GasToken, LedgerContract, NeoToken, Notary, OracleContract,
        PolicyContract, RoleManagement, StdLib, Treasury,
    };

    #[test]
    fn native_contract_handle_hash_accessors_are_uniform() {
        assert_eq!(
            ContractManagement::new().hash(),
            ContractManagement::script_hash()
        );
        assert_eq!(StdLib::new().hash(), StdLib::script_hash());
        assert_eq!(CryptoLib::new().hash(), CryptoLib::script_hash());
        assert_eq!(LedgerContract::new().hash(), LedgerContract::script_hash());
        assert_eq!(NeoToken::new().hash(), NeoToken::script_hash());
        assert_eq!(GasToken::new().hash(), GasToken::script_hash());
        assert_eq!(PolicyContract::new().hash(), PolicyContract::script_hash());
        assert_eq!(RoleManagement::new().hash(), RoleManagement::script_hash());
        assert_eq!(OracleContract::new().hash(), OracleContract::script_hash());
        assert_eq!(Notary::new().hash(), Notary::script_hash());
        assert_eq!(Treasury::new().hash(), Treasury::script_hash());
    }
}

#[test]
fn standard_native_contract_catalog_matches_provider_order_and_metadata() {
    let specs = standard_native_contract_specs();
    let provider = StandardNativeProvider::new();
    let contracts = provider.all_native_contracts();

    assert_eq!(contracts.len(), specs.len());
    assert_eq!(
        specs.iter().map(|spec| spec.id).collect::<Vec<_>>(),
        (-11..=-1).rev().collect::<Vec<_>>()
    );

    for (contract, spec) in contracts.iter().zip(specs.iter()) {
        assert_eq!(contract.id(), spec.id);
        assert_eq!(contract.name(), spec.name);
        assert_eq!(contract.hash(), spec.hash);
        assert_eq!(
            provider
                .get_native_contract(&spec.hash)
                .expect("hash resolves")
                .name(),
            spec.name
        );
        assert_eq!(
            provider
                .get_native_contract_by_name(spec.name)
                .expect("name resolves")
                .hash(),
            spec.hash
        );
    }
}

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

    // Notary declares no [ContractEvent] in C# v3.9.1.
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

    // LedgerContract declares no [ContractEvent] in C# v3.9.1.
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

    // StdLib declares no [ContractEvent] in C# v3.9.1.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}

#[test]
fn crypto_lib_manifest_pins_csharp_metadata() {
    let settings = test_settings();
    let contract = CryptoLib::new();

    // All hardforks active: verifyWithECDsa is the Cockatrice V1 whose
    // fourth C# parameter is `curveHash`.
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

    // CryptoLib declares no [ContractEvent] in C# v3.9.1.
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

    // Treasury declares no [ContractEvent] in C# v3.9.1.
    assert_eq!(manifest_events(&contract, &settings, ALL_ACTIVE), vec![]);
}

/// Every Rust `NativeMethod` (raw tables, including hardfork-gated dual
/// registrations that never co-exist in one manifest) must satisfy the C#
/// derivation `Safe = (RequiredCallFlags & ~CallFlags.ReadOnly) == 0`
/// (ContractMethodMetadata.cs:74). C# cannot express a safe flag that
/// disagrees with the method's call flags; this closes the same degree of
/// freedom in the hand-maintained Rust tables.
#[test]
fn native_method_safe_flags_follow_csharp_derivation() {
    let contracts = StandardNativeProvider::new().all_native_contracts();
    for contract in &contracts {
        for method in contract.methods() {
            let derived = method.required_call_flags & !CallFlags::READ_ONLY.bits() == 0;
            assert_eq!(
                method.safe,
                derived,
                "{}::{}/{}: safe={} but RequiredCallFlags={:#04x} derives safe={} \
                 (C# ContractMethodMetadata.cs:74)",
                contract.name(),
                method.name,
                method.parameters.len(),
                method.safe,
                method.required_call_flags,
                derived,
            );
        }
    }
}

/// Pins the SAFE-method set of every composed native manifest against the
/// literal C# `[ContractMethod]` attributes. Combined with the full method
/// lists pinned per contract above, this fixes every method's manifest
/// `safe` flag: safe iff RequiredCallFlags ⊆ ReadStates|AllowCall.
#[test]
fn native_manifest_safe_flags_pin_csharp_attributes() {
    let settings = test_settings();

    // GasToken (all from FungibleToken.cs): symbol/decimals `[ContractMethod]`
    // (RequiredCallFlags defaults to None), totalSupply/balanceOf ReadStates;
    // transfer is States|AllowCall|AllowNotify -> not safe.
    assert_eq!(
        manifest_safe_methods(&GasToken::new(), &settings, ALL_ACTIVE),
        vec![
            s("balanceOf", 1),
            s("decimals", 0),
            s("symbol", 0),
            s("totalSupply", 0),
        ]
    );

    // NeoToken: every get*/unclaimedGas (incl. the getAllCandidates iterator)
    // is ReadStates plus the FungibleToken reads; the writers (set*,
    // registerCandidate, unregisterCandidate, vote, transfer, and the Echidna
    // onNEP17Payment) all carry States (± AllowNotify) -> not safe.
    assert_eq!(
        manifest_safe_methods(&NeoToken::new(), &settings, ALL_ACTIVE),
        vec![
            s("balanceOf", 1),
            s("decimals", 0),
            s("getAccountState", 1),
            s("getAllCandidates", 0),
            s("getCandidateVote", 1),
            s("getCandidates", 0),
            s("getCommittee", 0),
            s("getCommitteeAddress", 0),
            s("getGasPerBlock", 0),
            s("getNextBlockValidators", 0),
            s("getRegisterPrice", 0),
            s("symbol", 0),
            s("totalSupply", 0),
            s("unclaimedGas", 2),
        ]
    );

    // PolicyContract: all get*/isBlocked are ReadStates; every set*, both
    // blockAccount registrations, unblockAccount, the whitelist writers and
    // recoverFund carry States (± AllowNotify) -> not safe.
    assert_eq!(
        manifest_safe_methods(&PolicyContract::new(), &settings, ALL_ACTIVE),
        vec![
            s("getAttributeFee", 1),
            s("getBlockedAccounts", 0),
            s("getExecFeeFactor", 0),
            s("getExecPicoFeeFactor", 0),
            s("getFeePerByte", 0),
            s("getMaxTraceableBlocks", 0),
            s("getMaxValidUntilBlockIncrement", 0),
            s("getMillisecondsPerBlock", 0),
            s("getStoragePrice", 0),
            s("getWhitelistFeeContracts", 0),
            s("isBlocked", 1),
        ]
    );

    // ContractManagement: the lookups are ReadStates; deploy/update/destroy
    // are States|AllowNotify and setMinimumDeploymentFee States -> not safe.
    assert_eq!(
        manifest_safe_methods(&ContractManagement::new(), &settings, ALL_ACTIVE),
        vec![
            s("getContract", 1),
            s("getContractById", 1),
            s("getContractHashes", 0),
            s("getMinimumDeploymentFee", 0),
            s("hasMethod", 3),
            s("isContract", 1),
        ]
    );

    // OracleContract: getPrice is ReadStates and Verify is a bare
    // `[ContractMethod(CpuFee = 1 << 15)]` -> RequiredCallFlags None -> SAFE;
    // finish/request/setPrice carry States -> not safe.
    assert_eq!(
        manifest_safe_methods(&OracleContract::new(), &settings, ALL_ACTIVE),
        vec![s("getPrice", 0), s("verify", 0)]
    );

    // RoleManagement: getDesignatedByRole ReadStates; designateAsRole
    // States|AllowNotify -> not safe.
    assert_eq!(
        manifest_safe_methods(&RoleManagement::new(), &settings, ALL_ACTIVE),
        vec![s("getDesignatedByRole", 2)]
    );

    // Notary: the deposit reads AND Verify
    // (`[ContractMethod(CpuFee = 1 << 15, RequiredCallFlags = CallFlags.ReadStates)]`,
    // Notary.cs) are safe; onNEP17Payment/lockDepositUntil/
    // setMaxNotValidBeforeDelta are States and withdraw is All -> not safe.
    assert_eq!(
        manifest_safe_methods(&Notary::new(), &settings, ALL_ACTIVE),
        vec![
            s("balanceOf", 1),
            s("expirationOf", 1),
            s("getMaxNotValidBeforeDelta", 0),
            s("verify", 1),
        ]
    );

    // LedgerContract: every method is ReadStates -> the whole manifest is safe.
    assert_eq!(
        manifest_safe_methods(&LedgerContract::new(), &settings, ALL_ACTIVE),
        vec![
            s("currentHash", 0),
            s("currentIndex", 0),
            s("getBlock", 1),
            s("getTransaction", 1),
            s("getTransactionFromBlock", 2),
            s("getTransactionHeight", 1),
            s("getTransactionSigners", 1),
            s("getTransactionVMState", 1),
        ]
    );

    // StdLib: every `[ContractMethod]` omits RequiredCallFlags (None) -> the
    // whole manifest is safe.
    assert_eq!(
        manifest_safe_methods(&StdLib::new(), &settings, ALL_ACTIVE),
        vec![
            s("atoi", 1),
            s("atoi", 2),
            s("base58CheckDecode", 1),
            s("base58CheckEncode", 1),
            s("base58Decode", 1),
            s("base58Encode", 1),
            s("base64Decode", 1),
            s("base64Encode", 1),
            s("base64UrlDecode", 1),
            s("base64UrlEncode", 1),
            s("deserialize", 1),
            s("hexDecode", 1),
            s("hexEncode", 1),
            s("itoa", 1),
            s("itoa", 2),
            s("jsonDeserialize", 1),
            s("jsonSerialize", 1),
            s("memoryCompare", 2),
            s("memorySearch", 2),
            s("memorySearch", 3),
            s("memorySearch", 4),
            s("serialize", 1),
            s("strLen", 1),
            s("stringSplit", 2),
            s("stringSplit", 3),
        ]
    );

    // CryptoLib: every `[ContractMethod]` omits RequiredCallFlags (None) ->
    // the whole manifest is safe.
    assert_eq!(
        manifest_safe_methods(&CryptoLib::new(), &settings, ALL_ACTIVE),
        vec![
            s("bls12381Add", 2),
            s("bls12381Deserialize", 1),
            s("bls12381Equal", 2),
            s("bls12381Mul", 3),
            s("bls12381Pairing", 2),
            s("bls12381Serialize", 1),
            s("keccak256", 1),
            s("murmur32", 2),
            s("recoverSecp256K1", 2),
            s("ripemd160", 1),
            s("sha256", 1),
            s("verifyWithECDsa", 4),
            s("verifyWithEd25519", 3),
        ]
    );

    // Treasury: both payment callbacks are
    // `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags = CallFlags.None)]`
    // (Treasury.cs) -> SAFE, unlike Notary's States-flagged onNEP17Payment;
    // verify is `CallFlags.ReadStates` (Treasury.cs:41) -> also SAFE.
    assert_eq!(
        manifest_safe_methods(&Treasury::new(), &settings, ALL_ACTIVE),
        vec![
            s("onNEP11Payment", 4),
            s("onNEP17Payment", 3),
            s("verify", 0)
        ]
    );
}
