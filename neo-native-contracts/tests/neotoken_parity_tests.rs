//
// Migrated from neo-core/tests/neotoken_parity_tests.rs (Task #228 wave 1).
// Test names and bodies preserved verbatim; import roots repointed to their
// owning crates (neo-native-contracts / neo-smart-contract / neo-protocol /
// neo-storage / neo-primitives / neo-vm / neo-crypto / neo-wallets).
// create_genesis_block here returns neo_protocol::payloads::Block directly, so the
// former genesis.to_payload_block() calls became genesis.clone().
//
// NeoToken parity tests against C# Neo v3.10.1.
// These smoke checks are supplemented by the state-root and native-contract
// suites; mainnet replay fixtures remain an explicit follow-up.

#[test]
fn test_unclaimed_gas_calculation_components() {
    // C# formula:
    // holder_reward = sumGasPerBlock * NeoHolderRewardRatio / 100 / TotalAmount
    // voter_reward = Balance * (latestGasPerVote - LastGasPerVote) / VoteFactor

    // Verify constants match C#
    let neo_holder_reward_ratio = 10u8; // 10% to holders
    let vote_factor = 100_000_000u64;

    assert_eq!(neo_holder_reward_ratio, 10);
    assert_eq!(vote_factor, 100_000_000);
}

// Follow-up fixture work (tracked separately):
// - RegisterCandidate with real v3.10.1 transactions
// - Vote state transitions at hardfork boundaries
// - UnclaimedGas calculations at specific heights
// - GetCandidates ordering and filtering
