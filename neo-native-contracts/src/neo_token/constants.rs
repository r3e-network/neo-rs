//! NeoToken protocol constants and event names.
//!
//! Centralizes storage prefixes, default governance values, reward ratios, and
//! native event names so the contract root stays focused on the native-contract
//! surface.

/// C# `NeoToken.Prefix_RegisterPrice`.
pub(in crate::neo_token) const PREFIX_REGISTER_PRICE: u8 = 13;
/// C# default candidate register price: 1000 GAS, in datoshi (1000 * 1e8).
pub(in crate::neo_token) const DEFAULT_REGISTER_PRICE: i64 = 1000 * 100_000_000;
/// C# `NeoToken.Prefix_GasPerBlock`.
pub(in crate::neo_token) const PREFIX_GAS_PER_BLOCK: u8 = 29;
/// C# default GAS-per-block at index 0: 5 GAS, in datoshi (5 * 1e8).
pub(in crate::neo_token) const DEFAULT_GAS_PER_BLOCK: i64 = 5 * 100_000_000;
/// C# `NeoToken.Prefix_Committee` - the cached `(pubkey, votes)` committee list.
pub(in crate::neo_token) const PREFIX_COMMITTEE: u8 = 14;
/// C# `NeoToken.Prefix_Candidate` - per-candidate `(Registered, Votes)` state.
pub(in crate::neo_token) const PREFIX_CANDIDATE: u8 = 33;
/// C# `NeoToken.Prefix_VoterRewardPerCommittee` - accumulated GAS-per-vote.
pub(in crate::neo_token) const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;
/// C# `NeoToken.Prefix_VotersCount` - total NEO that has voted (a BigInteger).
pub(in crate::neo_token) const PREFIX_VOTERS_COUNT: u8 = 1;
/// C# `NeoToken.NeoHolderRewardRatio` (10%).
pub(in crate::neo_token) const NEO_HOLDER_REWARD_RATIO: i64 = 10;
/// C# `NeoToken.CommitteeRewardRatio` (10%): the per-block GAS share minted to
/// the committee member selected by `index % committeeCount`.
pub(in crate::neo_token) const COMMITTEE_REWARD_RATIO: i64 = 10;
/// C# `NeoToken.VoterRewardRatio` (80%): the GAS share accrued (on committee
/// refresh blocks) to the voters of the committee.
pub(in crate::neo_token) const VOTER_REWARD_RATIO: i64 = 80;
/// C# `NeoToken.VoteFactor` (1e8): the zoom factor for per-vote GAS rewards.
pub(in crate::neo_token) const VOTE_FACTOR: i64 = 100_000_000;
/// C# `NeoToken.TotalAmount` = 100,000,000 NEO (decimals 0, so Factor = 1).
pub(in crate::neo_token) const NEO_TOTAL_AMOUNT: i64 = 100_000_000;

pub(crate) const NEO_CANDIDATE_STATE_CHANGED_EVENT: &str = "CandidateStateChanged";
pub(crate) const NEO_VOTE_EVENT: &str = "Vote";
pub(crate) const NEO_COMMITTEE_CHANGED_EVENT: &str = "CommitteeChanged";
