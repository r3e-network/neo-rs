//! NeoToken (NEO) native contract (id -5).
//!
//! Implements the NEP-17 metadata of the C# `Neo.SmartContract.Native.NeoToken`
//! (`symbol` "NEO", `decimals` 0). NEO's stateful surface (NEP-17 balances plus
//! governance: vote, candidates, committee, getGasPerBlock, unclaimedGas, ...)
//! is the next increment on the storage-backed pattern; the methods declared
//! below are byte-for-byte C# parity.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::StorageKey;
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;

use crate::hashes::NEO_TOKEN_HASH;
use crate::LedgerContract;

/// C# `NeoToken.Prefix_RegisterPrice`.
const PREFIX_REGISTER_PRICE: u8 = 13;
/// C# default candidate register price: 1000 GAS, in datoshi (1000 * 1e8).
const DEFAULT_REGISTER_PRICE: i64 = 1000 * 100_000_000;
/// C# `NeoToken.Prefix_GasPerBlock`.
const PREFIX_GAS_PER_BLOCK: u8 = 29;
/// C# default GAS-per-block at index 0: 5 GAS, in datoshi (5 * 1e8).
const DEFAULT_GAS_PER_BLOCK: i64 = 5 * 100_000_000;
/// C# `NeoToken.Prefix_Committee` — the cached `(pubkey, votes)` committee list.
const PREFIX_COMMITTEE: u8 = 14;
/// C# `NeoToken.Prefix_Candidate` — per-candidate `(Registered, Votes)` state.
const PREFIX_CANDIDATE: u8 = 33;

/// Lazily-initialised script-hash handle for the NEO native contract.
pub static NEO_HASH: LazyLock<UInt160> = LazyLock::new(|| *NEO_TOKEN_HASH);

/// The NeoToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeoToken;

impl NeoToken {
    /// Stable native contract id (matches C# `NeoToken`).
    pub const ID: i32 = -5;
    /// NEP-17 symbol (C# `NeoToken.Symbol => "NEO"`).
    pub const SYMBOL: &'static str = "NEO";
    /// NEP-17 decimals (C# `NeoToken.Decimals => 0`).
    pub const DECIMALS: u8 = 0;

    /// Construct a new `NeoToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the NEO script hash.
    pub fn script_hash() -> UInt160 {
        *NEO_HASH
    }
}

/// C# `GetRegisterPrice` = `(long)(BigInteger)snapshot[_registerPrice]`.
fn register_price(snapshot: &DataCache) -> CoreResult<i64> {
    crate::read_storage_int(
        snapshot,
        NeoToken::ID,
        PREFIX_REGISTER_PRICE,
        DEFAULT_REGISTER_PRICE,
    )
}

/// Returns the GAS-per-block effective at `index`: the most recent
/// `Prefix_GasPerBlock` record whose record index is ≤ `index` (C#
/// `GetSortedGasRecords(...).First().GasPerBlock`), defaulting to 5 GAS.
fn gas_per_block_at(snapshot: &DataCache, index: u32) -> BigInt {
    let prefix = StorageKey::new(NeoToken::ID, vec![PREFIX_GAS_PER_BLOCK]);
    for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Backward) {
        let key_bytes = key.key();
        if key_bytes.len() >= 5 {
            let record_index =
                u32::from_be_bytes([key_bytes[1], key_bytes[2], key_bytes[3], key_bytes[4]]);
            if record_index <= index {
                return BigInt::from_signed_bytes_le(&item.value_bytes());
            }
        }
    }
    BigInt::from(DEFAULT_GAS_PER_BLOCK)
}

/// Reads the cached committee public keys from `Prefix_Committee` (C#
/// `GetCommitteeFromCache`). The value is a `BinarySerializer` array whose
/// elements are `Struct[pubkey(33-byte compressed), votes]` (C#
/// `CachedCommittee.ElementToStackItem`); only the public keys are returned, in
/// stored order.
fn read_committee_points(snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
    let key = StorageKey::new(NeoToken::ID, vec![PREFIX_COMMITTEE]);
    let item = snapshot.get(&key).ok_or_else(|| {
        CoreError::invalid_operation("NeoToken committee cache is not initialized")
    })?;
    let decoded = BinarySerializer::deserialize(&item.value_bytes(), &ExecutionEngineLimits::default(), None)
        .map_err(|e| CoreError::deserialization(format!("committee cache: {e}")))?;
    let StackItem::Array(array) = decoded else {
        return Err(CoreError::invalid_data("committee cache is not an array"));
    };
    let mut points = Vec::with_capacity(array.items().len());
    for element in array.items() {
        let StackItem::Struct(fields) = element else {
            return Err(CoreError::invalid_data("committee element is not a struct"));
        };
        let items = fields.items();
        let pubkey = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("committee element is empty"))?;
        let bytes = pubkey
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("committee pubkey: {e}")))?;
        points.push(
            ECPoint::from_bytes(&bytes)
                .map_err(|e| CoreError::invalid_data(format!("committee EC point: {e}")))?,
        );
    }
    Ok(points)
}

/// C# `GetCommittee` = committee public keys sorted ascending (`OrderBy(p => p)`).
fn committee_sorted(snapshot: &DataCache) -> CoreResult<Vec<ECPoint>> {
    let mut points = read_committee_points(snapshot)?;
    points.sort();
    Ok(points)
}

/// C# `GetNextBlockValidators`: the first `validators_count` committee members
/// (in stored, vote-ranked order), then sorted ascending.
fn next_block_validators(snapshot: &DataCache, validators_count: usize) -> CoreResult<Vec<ECPoint>> {
    let mut points = read_committee_points(snapshot)?;
    points.truncate(validators_count);
    points.sort();
    Ok(points)
}

/// C# `GetCandidates` (= `GetCandidatesInternal.Where(Registered)`): scan
/// `Prefix_Candidate` (key = prefix ++ 33-byte pubkey; value = CandidateState
/// `Struct[Registered(bool), Votes]`), returning the `(pubkey, votes)` pairs of
/// the registered candidates in storage-scan order.
fn read_registered_candidates(snapshot: &DataCache) -> CoreResult<Vec<(ECPoint, BigInt)>> {
    let prefix = StorageKey::new(NeoToken::ID, vec![PREFIX_CANDIDATE]);
    let mut out = Vec::new();
    for (key, item) in snapshot.find(Some(&prefix), SeekDirection::Forward) {
        let key_bytes = key.key();
        if key_bytes.len() < 34 {
            continue;
        }
        let Ok(pubkey) = ECPoint::from_bytes(&key_bytes[1..34]) else {
            continue;
        };
        let decoded = BinarySerializer::deserialize(
            &item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .map_err(|e| CoreError::deserialization(format!("candidate state: {e}")))?;
        let StackItem::Struct(fields) = decoded else {
            return Err(CoreError::invalid_data("candidate state is not a struct"));
        };
        let items = fields.items();
        let registered = items.first().is_some_and(|f| f.as_bool().unwrap_or(false));
        if !registered {
            continue;
        }
        let votes = match items.get(1) {
            Some(f) => f
                .as_int()
                .map_err(|e| CoreError::invalid_data(format!("candidate votes: {e}")))?,
            None => BigInt::from(0),
        };
        out.push((pubkey, votes));
    }
    Ok(out)
}

/// Marshals `(pubkey, votes)` candidate pairs as an Array of `Struct[pubkey,
/// votes]` (C# `(ECPoint, BigInteger)[]` return shape).
fn candidates_to_array_bytes(candidates: &[(ECPoint, BigInt)]) -> CoreResult<Vec<u8>> {
    let array = StackItem::from_array(
        candidates
            .iter()
            .map(|(pk, votes)| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(pk.to_bytes()),
                    StackItem::from_int(votes.clone()),
                ])
            })
            .collect::<Vec<_>>(),
    );
    BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("getCandidates: {e}")))
}

/// Serializes EC points as an Array of compressed (33-byte) byte strings — the
/// return shape shared by `getCommittee` / `getNextBlockValidators`.
fn points_to_array_bytes(points: &[ECPoint]) -> CoreResult<Vec<u8>> {
    let array = StackItem::from_array(
        points
            .iter()
            .map(|p| StackItem::from_byte_string(p.to_bytes()))
            .collect::<Vec<_>>(),
    );
    BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::invalid_operation(format!("NeoToken point array: {e}")))
}

/// The committee multisig threshold `m = n - (n - 1) / 2` (committee majority,
/// matching C# `GetCommitteeAddress`). `n` must be non-zero.
fn committee_threshold(n: usize) -> usize {
    n - (n - 1) / 2
}

/// C# `GetCommitteeAddress` = script hash of the `m`-of-`n` multisig over the
/// committee public keys, where `m = n - (n - 1) / 2`. The multisig builder sorts
/// the keys ascending exactly as C# `Contract.CreateMultiSigRedeemScript` does.
fn compute_committee_address(snapshot: &DataCache) -> CoreResult<UInt160> {
    let points = read_committee_points(snapshot)?;
    if points.is_empty() {
        return Err(CoreError::invalid_operation("committee is empty"));
    }
    let m = committee_threshold(points.len());
    let script = neo_redeem_script::multi_sig_redeem_script_from_points(m, &points)
        .map_err(|e| CoreError::invalid_operation(format!("committee multisig script: {e}")))?;
    Ok(UInt160::from_script(&script))
}

/// C# `GetAccountState`: the stored `NeoAccountState` struct bytes under
/// `Prefix_Account ++ account`, or `None` when the account has no entry. The
/// stored value is already the BinarySerializer-encoded struct (balance,
/// balanceHeight, voteTo, lastGasPerVote), which is exactly the Array/Struct
/// return shape — so it is returned as-is (the same pattern as
/// `getDesignatedByRole` / `getContract`).
fn read_account_state(snapshot: &DataCache, account: &UInt160) -> Option<Vec<u8>> {
    let mut key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
    key_bytes.extend_from_slice(&account.to_bytes());
    let key = StorageKey::new(NeoToken::ID, key_bytes);
    snapshot.get(&key).map(|item| item.value_bytes().into_owned())
}

static NEO_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new("symbol".into(), 0, true, 0, vec![], ContractParameterType::String),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], int),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethod::new("totalSupply".into(), 1 << 15, true, read_states, vec![], int),
        NativeMethod::new(
            "balanceOf".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        ),
        // Governance reads.
        NativeMethod::new("getGasPerBlock".into(), 1 << 15, true, read_states, vec![], int),
        NativeMethod::new("getRegisterPrice".into(), 1 << 15, true, read_states, vec![], int),
        // Committee reads (CpuFee 1<<16 in C#).
        NativeMethod::new(
            "getCommittee".into(),
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        NativeMethod::new(
            "getCommitteeAddress".into(),
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Hash160,
        )
        .with_active_in(Hardfork::HfCockatrice),
        // getAccountState(account) -> NeoAccountState struct (Array) or null.
        NativeMethod::new(
            "getAccountState".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Array,
        ),
        // getNextBlockValidators -> ECPoint[] (Array), CpuFee 1<<16 in C#.
        NativeMethod::new(
            "getNextBlockValidators".into(),
            1 << 16,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
        // getCandidates -> (ECPoint, BigInteger)[] (Array of Structs), CpuFee 1<<22.
        NativeMethod::new(
            "getCandidates".into(),
            1 << 22,
            true,
            read_states,
            vec![],
            ContractParameterType::Array,
        ),
    ]
});

impl NativeContract for NeoToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *NEO_HASH
    }

    fn name(&self) -> &str {
        "NeoToken"
    }

    fn methods(&self) -> &[NativeMethod] {
        &NEO_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// C# `NEO.GetCommitteeAddress`, exposed through the native-contract seam so
    /// the engine's `check_committee_witness` can verify committee-gated writers
    /// without depending on `neo-native-contracts`.
    fn committee_address(&self, snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        Ok(Some(compute_committee_address(snapshot)?))
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le()),
            "totalSupply" => {
                let snapshot = engine.snapshot_cache();
                let total =
                    crate::read_storage_int(&snapshot, Self::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY, 0)?;
                Ok(BigInt::from(total).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::balanceOf requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("NeoToken::balanceOf: bad account: {e}"))
                })?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            "getGasPerBlock" => {
                let snapshot = engine.snapshot_cache();
                let index = LedgerContract::new().current_index(&snapshot)?.saturating_add(1);
                Ok(gas_per_block_at(&snapshot, index).to_signed_bytes_le())
            }
            "getRegisterPrice" => {
                let snapshot = engine.snapshot_cache();
                Ok(BigInt::from(register_price(&snapshot)?).to_signed_bytes_le())
            }
            "getCommittee" => {
                // C# returns ECPoint[] sorted ascending; marshaled as an Array of
                // compressed (33-byte) public-key byte strings.
                let snapshot = engine.snapshot_cache();
                points_to_array_bytes(&committee_sorted(&snapshot)?)
            }
            "getNextBlockValidators" => {
                // First ValidatorsCount committee members (stored order), sorted.
                let count =
                    usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
                let snapshot = engine.snapshot_cache();
                points_to_array_bytes(&next_block_validators(&snapshot, count)?)
            }
            "getCandidates" => {
                let snapshot = engine.snapshot_cache();
                candidates_to_array_bytes(&read_registered_candidates(&snapshot)?)
            }
            "getCommitteeAddress" => {
                let snapshot = engine.snapshot_cache();
                Ok(compute_committee_address(&snapshot)?.to_bytes())
            }
            "getAccountState" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("NeoToken::getAccountState requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "NeoToken::getAccountState: bad account: {e}"
                    ))
                })?;
                let snapshot = engine.snapshot_cache();
                // C# returns the NeoAccountState struct, or null (empty payload)
                // when the account has no entry.
                Ok(read_account_state(&snapshot, &account).unwrap_or_default())
            }
            other => Err(CoreError::invalid_operation(format!(
                "NeoToken method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = NeoToken::new();
        assert_eq!(NativeContract::id(&c), -5);
        assert_eq!(NativeContract::name(&c), "NeoToken");
        assert_eq!(NativeContract::hash(&c), *NEO_TOKEN_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "symbol",
                "decimals",
                "totalSupply",
                "balanceOf",
                "getGasPerBlock",
                "getRegisterPrice",
                "getCommittee",
                "getCommitteeAddress",
                "getAccountState",
                "getNextBlockValidators",
                "getCandidates"
            ]
        );
        let acct = c.methods().iter().find(|m| m.name == "getAccountState").unwrap();
        assert_eq!(acct.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(acct.return_type, ContractParameterType::Array);
        assert_eq!(acct.cpu_fee, 1 << 15);
        let nbv = c.methods().iter().find(|m| m.name == "getNextBlockValidators").unwrap();
        assert_eq!(nbv.return_type, ContractParameterType::Array);
        assert_eq!(nbv.cpu_fee, 1 << 16);
        assert!(nbv.parameters.is_empty());
        let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
        assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
        let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
        assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());

        let committee = c.methods().iter().find(|m| m.name == "getCommittee").unwrap();
        assert_eq!(committee.cpu_fee, 1 << 16);
        assert_eq!(committee.return_type, ContractParameterType::Array);
        assert!(committee.active_in.is_none());
        let addr = c.methods().iter().find(|m| m.name == "getCommitteeAddress").unwrap();
        assert_eq!(addr.cpu_fee, 1 << 16);
        assert_eq!(addr.return_type, ContractParameterType::Hash160);
        assert_eq!(addr.active_in, Some(Hardfork::HfCockatrice));
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    /// Stores a committee cache (Array of `Struct[pubkey, votes]`) under
    /// `Prefix_Committee`, mirroring C# `CachedCommittee.ToStackItem`.
    fn seed_committee(cache: &DataCache, points: &[ECPoint]) {
        use neo_storage::StorageItem;
        let array = StackItem::from_array(
            points
                .iter()
                .map(|p| {
                    StackItem::from_struct(vec![
                        StackItem::from_byte_string(p.to_bytes()),
                        StackItem::from_int(0),
                    ])
                })
                .collect::<Vec<_>>(),
        );
        let bytes =
            BinarySerializer::serialize(&array, &ExecutionEngineLimits::default()).unwrap();
        cache.add(
            StorageKey::new(NeoToken::ID, vec![PREFIX_COMMITTEE]),
            StorageItem::from_bytes(bytes),
        );
    }

    fn sample_committee() -> Vec<ECPoint> {
        // Three valid secp256r1 public keys (Neo N3 standby validators).
        [
            "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c",
            "02df48f60e8f3e01c48ff40b9b7f1310d7a8b2a193188befe1c2e3df740e895093",
            "03b8d9d5771d8f513aa0869b9cc8d50986403b78c6da36890638c3d46a5adce04a",
        ]
        .iter()
        .map(|h| ECPoint::from_bytes(&hex(h)).unwrap())
        .collect()
    }

    #[test]
    fn committee_threshold_is_majority() {
        // m = n - (n - 1) / 2.
        assert_eq!(committee_threshold(1), 1);
        assert_eq!(committee_threshold(3), 2);
        assert_eq!(committee_threshold(4), 3);
        assert_eq!(committee_threshold(7), 4);
        assert_eq!(committee_threshold(21), 11);
    }

    #[test]
    fn committee_read_decodes_and_sorts() {
        let cache = DataCache::new(false);
        let points = sample_committee();
        seed_committee(&cache, &points);

        // Decoded points round-trip (stored order).
        let read = read_committee_points(&cache).unwrap();
        assert_eq!(read, points);

        // getCommittee returns them sorted ascending (C# OrderBy).
        let mut expected = points.clone();
        expected.sort();
        assert_eq!(committee_sorted(&cache).unwrap(), expected);
    }

    #[test]
    fn next_block_validators_takes_count_then_sorts() {
        let cache = DataCache::new(false);
        let points = sample_committee(); // 3 stored points
        seed_committee(&cache, &points);

        // Take the first 2 (stored order), then sort ascending.
        let result = next_block_validators(&cache, 2).unwrap();
        let mut expected: Vec<ECPoint> = points[..2].to_vec();
        expected.sort();
        assert_eq!(result, expected);

        // A count >= committee size returns all members, sorted.
        let mut all_expected = points.clone();
        all_expected.sort();
        assert_eq!(next_block_validators(&cache, 10).unwrap(), all_expected);
    }

    #[test]
    fn candidates_filters_registered_and_decodes_votes() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let points = sample_committee(); // 3 valid points

        // p0 registered w/ 100 votes, p1 unregistered, p2 registered w/ 50 votes.
        for (pk, registered, votes) in [
            (&points[0], true, 100i64),
            (&points[1], false, 0),
            (&points[2], true, 50),
        ] {
            let state = StackItem::from_struct(vec![
                StackItem::from_bool(registered),
                StackItem::from_int(votes),
            ]);
            let bytes =
                BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
            let mut key = vec![PREFIX_CANDIDATE];
            key.extend_from_slice(&pk.to_bytes());
            cache.add(StorageKey::new(NeoToken::ID, key), StorageItem::from_bytes(bytes));
        }

        let candidates = read_registered_candidates(&cache).unwrap();
        // Only the two registered candidates are returned.
        assert_eq!(candidates.len(), 2);
        let by_key: std::collections::HashMap<Vec<u8>, BigInt> =
            candidates.iter().map(|(pk, v)| (pk.to_bytes(), v.clone())).collect();
        assert_eq!(by_key.get(&points[0].to_bytes()), Some(&BigInt::from(100)));
        assert_eq!(by_key.get(&points[2].to_bytes()), Some(&BigInt::from(50)));
        assert!(!by_key.contains_key(&points[1].to_bytes()));
    }

    #[test]
    fn committee_address_matches_multisig_script_hash() {
        let cache = DataCache::new(false);
        let points = sample_committee();
        seed_committee(&cache, &points);

        // For n=3, m=2; the address is the 2-of-3 multisig script hash. The
        // builder sorts the keys the same way C# CreateMultiSigRedeemScript does.
        let script = neo_redeem_script::multi_sig_redeem_script_from_points(2, &points).unwrap();
        assert_eq!(compute_committee_address(&cache).unwrap(), UInt160::from_script(&script));
    }

    #[test]
    fn committee_address_uninitialized_errors() {
        // C# indexes snapshot[Prefix_Committee] and throws when absent.
        let cache = DataCache::new(false);
        assert!(compute_committee_address(&cache).is_err());
        assert!(read_committee_points(&cache).is_err());
    }

    #[test]
    fn committee_address_trait_override_feeds_the_engine_seam() {
        // The `NativeContract::committee_address` override is what the engine's
        // check_committee_witness reaches through the provider seam; it must
        // return the computed address (Some), and fault on a missing committee.
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let neo = NeoToken::new();
        assert_eq!(
            NativeContract::committee_address(&neo, &cache).unwrap(),
            Some(compute_committee_address(&cache).unwrap())
        );
        assert!(NativeContract::committee_address(&neo, &DataCache::new(false)).is_err());
    }

    #[test]
    fn balance_of_absent_account_is_zero() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[2u8; 20]).unwrap();
        assert_eq!(
            crate::read_nep17_balance(&cache, NeoToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
    }

    #[test]
    fn governance_reads_have_defaults_and_read_storage() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);

        // Defaults when unset: 1000 GAS register price, 5 GAS per block.
        assert_eq!(register_price(&cache).unwrap(), DEFAULT_REGISTER_PRICE);
        assert_eq!(gas_per_block_at(&cache, 100), BigInt::from(DEFAULT_GAS_PER_BLOCK));

        // register price reads the prefix-13 BigInteger.
        cache.add(
            StorageKey::new(NeoToken::ID, vec![PREFIX_REGISTER_PRICE]),
            StorageItem::from_bytes(BigInt::from(500 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(register_price(&cache).unwrap(), 500 * 100_000_000);

        // gas-per-block backward seek: record at index 10 applies from 10 on.
        let mut key = vec![PREFIX_GAS_PER_BLOCK];
        key.extend_from_slice(&10u32.to_be_bytes());
        cache.add(
            StorageKey::new(NeoToken::ID, key),
            StorageItem::from_bytes(BigInt::from(3 * 100_000_000i64).to_signed_bytes_le()),
        );
        assert_eq!(gas_per_block_at(&cache, 9), BigInt::from(DEFAULT_GAS_PER_BLOCK));
        assert_eq!(gas_per_block_at(&cache, 20), BigInt::from(3 * 100_000_000i64));
    }

    #[test]
    fn account_state_returns_stored_struct_or_none() {
        use neo_storage::StorageItem;
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[5u8; 20]).unwrap();

        // Absent -> None (invoke maps it to an empty payload = null).
        assert!(read_account_state(&cache, &account).is_none());

        // Store a NeoAccountState struct [balance, height, voteTo(Null),
        // lastGasPerVote] and read its raw bytes back unchanged.
        let state = StackItem::from_struct(vec![
            StackItem::from_int(123),
            StackItem::from_int(7),
            StackItem::null(),
            StackItem::from_int(0),
        ]);
        let bytes = BinarySerializer::serialize(&state, &ExecutionEngineLimits::default()).unwrap();
        let mut key_bytes = vec![crate::NEP17_PREFIX_ACCOUNT];
        key_bytes.extend_from_slice(&account.to_bytes());
        cache.add(
            StorageKey::new(NeoToken::ID, key_bytes),
            StorageItem::from_bytes(bytes.clone()),
        );
        assert_eq!(read_account_state(&cache, &account), Some(bytes.clone()));
        // The returned bytes deserialize to the 4-field struct.
        match BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap()
        {
            StackItem::Struct(s) => assert_eq!(s.items().len(), 4),
            other => panic!("expected Struct, got {other:?}"),
        }
    }
}
