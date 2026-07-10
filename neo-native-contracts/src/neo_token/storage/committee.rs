//! Committee cache readers and address derivation for NEO.
//!
//! Committee storage is hot during block persistence, witness checks, and
//! fast-sync catch-up. This module keeps the byte-keyed caches and canonical
//! stack-item fast paths together while preserving the C#-compatible
//! `Prefix_Committee` storage value.

use super::*;
use neo_error::CoreError;

/// Process-global memoization for the deserialized committee, keyed by the exact
/// `Prefix_Committee` storage bytes. A pure function of those bytes (same bytes
/// always deserialize to the same members), so it is correct across snapshots,
/// heights, and reverts. Eliminates the per-block EC-point decompression of the
/// committee pubkeys on the hot path. See [`NeoToken::read_committee_with_votes`].
static COMMITTEE_DESERIALIZE_CACHE: std::sync::Mutex<Option<(Vec<u8>, Vec<(ECPoint, BigInt)>)>> =
    std::sync::Mutex::new(None);

/// Process-global memoization for `GetCommitteeAddress`, keyed by the exact
/// `Prefix_Committee` storage bytes. The multisig address is a pure function of
/// those bytes; a committee refresh changes the bytes and therefore misses this
/// cache without any explicit invalidation hook.
static COMMITTEE_ADDRESS_CACHE: std::sync::Mutex<Option<(Vec<u8>, UInt160)>> =
    std::sync::Mutex::new(None);

/// Cache for the sorted next-block validator signature accounts.
///
/// `GasToken::on_persist` needs only the primary validator's account. This
/// cache avoids cloning/sorting the same committee and re-reading the same
/// signature accounts on every block while the `Prefix_Committee` bytes stay
/// unchanged.
static NEXT_VALIDATOR_ACCOUNTS_CACHE: std::sync::Mutex<Option<(Vec<u8>, usize, Vec<UInt160>)>> =
    std::sync::Mutex::new(None);

impl NeoToken {
    /// Reads the cached committee from `Prefix_Committee` (C#
    /// `GetCommitteeFromCache`) as `(pubkey, votes)` pairs in stored order. The
    /// value is a `BinarySerializer` array whose elements are `Struct[pubkey(33-byte
    /// compressed), votes]` (C# `CachedCommittee.ElementToStackItem`). Errors when
    /// the cache has never been initialized, matching the C# indexer/`GetAndChange`
    /// null deref.
    pub(in crate::neo_token) fn read_committee_with_votes<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<(ECPoint, BigInt)>> {
        let key = Self::committee_key();
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let raw = item.value_bytes();

        // Memoize the deserialized committee keyed by the exact stored bytes.
        // `read_committee_with_votes` is on the per-block hot path (GasToken
        // OnPersist primary reward, extensible-witness whitelist), and each
        // deserialization EC-point-decompresses all committee pubkeys - the
        // single dominant CPU cost during catch-up. The committee bytes only
        // change on a refresh block (every `committee_count` blocks), so this is
        // a pure function of the bytes (same bytes => same members): correct
        // across snapshots/heights/reverts, mirroring C#'s in-memory committee
        // cache (`GetCommitteeFromCache`).
        {
            let cache = COMMITTEE_DESERIALIZE_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some((cached_bytes, cached_members)) = cache.as_ref() {
                if cached_bytes.as_slice() == raw.as_ref() {
                    return Ok(cached_members.clone());
                }
            }
        }

        let decoded = crate::support::codec::decode_stack_value(&raw, "committee cache")?;
        let members = CachedCommittee::from_stack_value(decoded)?.into_members();

        let mut cache = COMMITTEE_DESERIALIZE_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cache = Some((raw.into_owned(), members.clone()));
        Ok(members)
    }

    /// Reads only the cached committee public keys, in stored order.
    pub(in crate::neo_token) fn read_committee_points<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<ECPoint>> {
        Ok(self
            .read_committee_with_votes(snapshot)?
            .into_iter()
            .map(|(point, _)| point)
            .collect())
    }

    pub(crate) fn next_block_validator_account<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        validators_count: usize,
        primary_index: usize,
    ) -> CoreResult<UInt160> {
        let key = Self::committee_key();
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let raw = item.value_bytes();

        {
            let cache = NEXT_VALIDATOR_ACCOUNTS_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some((cached_bytes, cached_count, cached_accounts)) = cache.as_ref() {
                if *cached_count == validators_count && cached_bytes.as_slice() == raw.as_ref() {
                    return cached_accounts.get(primary_index).copied().ok_or_else(|| {
                        CoreError::invalid_operation(format!(
                            "NeoToken next-block validator primary index {primary_index} outside the validator set"
                        ))
                    });
                }
            }
        }

        let mut points = self.read_committee_points(snapshot)?;
        points.truncate(validators_count);
        points.sort();
        let accounts = points
            .iter()
            .map(candidate_signature_account)
            .collect::<Vec<_>>();
        let account = accounts.get(primary_index).copied().ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "NeoToken next-block validator primary index {primary_index} outside the validator set"
            ))
        })?;

        let mut cache = NEXT_VALIDATOR_ACCOUNTS_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cache = Some((raw.into_owned(), validators_count, accounts));
        Ok(account)
    }

    /// Reads a single cached committee member by stored index.
    ///
    /// The per-block `PostPersist` reward only needs one rotating committee
    /// member. Canonical committee bytes are parsed directly to avoid
    /// deserializing and cloning the whole committee vector on every block; any
    /// non-canonical-but-valid historical shape falls back to the generic reader.
    pub(in crate::neo_token) fn read_committee_member_at<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        index: usize,
    ) -> CoreResult<(ECPoint, BigInt)> {
        let key = Self::committee_key();
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let raw = item.value_bytes();
        if let Some(member) = Self::decode_canonical_committee_member_at(raw.as_ref(), index)? {
            return Ok(member);
        }
        self.read_committee_with_votes(snapshot)?
            .into_iter()
            .nth(index)
            .ok_or_else(|| CoreError::invalid_operation("NeoToken committee cache too small"))
    }

    /// Serializes `(pubkey, votes)` committee members as the `Prefix_Committee`
    /// storage value - an Array of `Struct[pubkey, votes]` (C#
    /// `CachedCommittee.ToStackItem`), the byte-exact write counterpart of
    /// [`read_committee_with_votes`].
    pub(in crate::neo_token) fn encode_committee(
        members: &[(ECPoint, BigInt)],
    ) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(
            &CachedCommittee::new(members.to_vec()),
            "committee cache",
        )
    }

    pub(in crate::neo_token) fn decode_canonical_committee_member_at(
        value: &[u8],
        index: usize,
    ) -> CoreResult<Option<(ECPoint, BigInt)>> {
        const ARRAY: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_ARRAY;
        const STRUCT: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_STRUCT;
        const BYTE_STRING: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_BYTESTRING;
        const INTEGER: u8 = neo_vm_rs::NEOVM_STACK_ITEM_TYPE_INTEGER;

        let Some((&ARRAY, tail)) = value.split_first() else {
            return Ok(None);
        };
        let Some((count, count_width)) = neo_io::var_int::VarInt::read_var_int_prefix(tail) else {
            return Ok(None);
        };
        if index >= count as usize {
            return Err(CoreError::invalid_operation(
                "NeoToken committee cache too small",
            ));
        }

        let mut tail = &tail[count_width..];
        let mut selected = None;
        for member_index in 0..count as usize {
            let Some((&STRUCT, rest)) = tail.split_first() else {
                return Ok(None);
            };
            let Some((&2, rest)) = rest.split_first() else {
                return Ok(None);
            };
            let Some((&BYTE_STRING, rest)) = rest.split_first() else {
                return Ok(None);
            };
            let Some((key_len, key_len_width)) = neo_io::var_int::VarInt::read_var_int_prefix(rest)
            else {
                return Ok(None);
            };
            if key_len != 33 {
                return Ok(None);
            }
            let key_start = key_len_width;
            let key_end = key_start + key_len as usize;
            if rest.len() < key_end {
                return Ok(None);
            }
            let pubkey_bytes = &rest[key_start..key_end];
            let rest = &rest[key_end..];
            let Some((&INTEGER, rest)) = rest.split_first() else {
                return Ok(None);
            };
            let Some((vote_len, vote_len_width)) =
                neo_io::var_int::VarInt::read_var_int_prefix(rest)
            else {
                return Ok(None);
            };
            if vote_len > 32 {
                return Ok(None);
            }
            let vote_start = vote_len_width;
            let vote_end = vote_start + vote_len as usize;
            if rest.len() < vote_end {
                return Ok(None);
            }
            if member_index == index {
                let point = ECPoint::from_bytes(pubkey_bytes)
                    .map_err(|e| CoreError::invalid_data(format!("committee EC point: {e}")))?;
                selected = Some((
                    point,
                    BigInt::from_signed_bytes_le(&rest[vote_start..vote_end]),
                ));
            }
            tail = &rest[vote_end..];
        }
        if !tail.is_empty() {
            return Ok(None);
        }
        Ok(selected)
    }

    /// C# `NeoToken.ShouldRefreshCommittee(height, committeeMembersCount)`:
    /// the committee is recounted on every block whose index is a multiple of the
    /// committee size. `committee_count` must be non-zero (validated by callers,
    /// like the C# division-by-zero).
    pub(in crate::neo_token) fn should_refresh_committee(
        height: u32,
        committee_count: usize,
    ) -> bool {
        height % (committee_count as u32) == 0
    }

    /// C# `Contract.GetBFTAddress(pubkeys)`: the script hash of the
    /// `m`-of-`n` multisig over `pubkeys` with the BFT threshold
    /// `m = n - (n - 1) / 3`. (Distinct from the committee address, whose
    /// threshold is the simple majority `n - (n - 1) / 2`.) `pub(crate)` so
    /// `GasToken::initialize` can mint the initial GAS distribution to the
    /// standby-validator BFT address (C# GasToken.cs:33).
    pub(crate) fn bft_address(pubkeys: &[ECPoint]) -> CoreResult<UInt160> {
        neo_vm::script_builder::RedeemScript::bft_address(pubkeys)
            .ok_or_else(|| CoreError::invalid_operation("BFT address requires at least one key"))
    }

    /// C# `GetCommittee` = committee public keys sorted ascending (`OrderBy(p => p)`).
    pub(in crate::neo_token) fn committee_sorted<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Vec<ECPoint>> {
        let mut points = self.read_committee_points(snapshot)?;
        points.sort();
        Ok(points)
    }

    /// The committee multisig threshold `m = n - (n - 1) / 2` (committee majority,
    /// matching C# `GetCommitteeAddress`). `n` must be non-zero. The single source
    /// of this term; `PolicyContract::assert_almost_full_committee` reuses it.
    pub(crate) fn committee_threshold(n: usize) -> usize {
        n - (n - 1) / 2
    }

    /// C# `GetCommitteeAddress` = script hash of the `m`-of-`n` multisig over the
    /// committee public keys, where `m = n - (n - 1) / 2`. The multisig builder sorts
    /// the keys ascending exactly as C# `Contract.CreateMultiSigRedeemScript` does.
    pub(in crate::neo_token) fn compute_committee_address<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<UInt160> {
        let key = Self::committee_key();
        let item = snapshot.get(&key).ok_or_else(|| {
            CoreError::invalid_operation("NeoToken committee cache is not initialized")
        })?;
        let raw = item.value_bytes();

        {
            let cache = COMMITTEE_ADDRESS_CACHE
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some((cached_bytes, cached_address)) = cache.as_ref() {
                if cached_bytes.as_slice() == raw.as_ref() {
                    return Ok(*cached_address);
                }
            }
        }

        let points = self.read_committee_points(snapshot)?;
        if points.is_empty() {
            return Err(CoreError::invalid_operation("committee is empty"));
        }
        let m = Self::committee_threshold(points.len());
        let script =
            neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
                m, &points,
            )
                .map_err(|e| CoreError::invalid_operation(format!("committee multisig script: {e}")))?;
        let address = UInt160::from_script(&script);

        let mut cache = COMMITTEE_ADDRESS_CACHE
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        *cache = Some((raw.into_owned(), address));
        Ok(address)
    }
}
