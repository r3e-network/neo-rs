use super::*;

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
