//! NeoToken native-method handlers.
//!
//! Keeps governance, voting, transfer, and candidate-registration method bodies
//! out of the contract root while preserving C#-compatible validation order,
//! fee accounting, storage writes, notifications, and payment-callback
//! semantics. Dispatch is declared by the metadata binding table and
//! `native_contract_dispatch!`.

use neo_config::Hardfork;
use neo_crypto::ECPoint;
use neo_error::{CoreError, CoreResult};
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_execution::{ApplicationEngine, Contract};
use neo_primitives::{FindOptions, UInt160};
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::LedgerContract;

use super::{NEO_CANDIDATE_STATE_CHANGED_EVENT, NEO_TOTAL_AMOUNT, NeoToken};

impl NeoToken {
    pub(super) fn invoke_symbol(
        &self,
        _engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Ok(Self::SYMBOL.as_bytes().to_vec())
    }

    pub(super) fn invoke_decimals(
        &self,
        _engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le())
    }

    pub(super) fn invoke_total_supply(
        &self,
        _engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# `NeoToken.TotalSupply` overrides the fungible-token storage
        // reader and returns the immutable protocol amount.
        Ok(BigInt::from(NEO_TOTAL_AMOUNT).to_signed_bytes_le())
    }

    pub(super) fn invoke_balance_of(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let account = crate::args::raw_account(args, "NeoToken::balanceOf")?;
        let snapshot = engine.snapshot_cache();
        Ok(self.balance_of(&snapshot, &account)?.to_signed_bytes_le())
    }

    pub(super) fn invoke_transfer(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# FungibleToken.Transfer(from, to, amount, data) with NEO's
        // governance OnBalanceChanging side-effects.
        let from = crate::args::raw_hash160(args, 0, "NeoToken::transfer")?;
        let to = crate::args::raw_hash160(args, 1, "NeoToken::transfer")?;
        let amount =
            crate::args::raw_required_integer_arg(args, 2, "NeoToken::transfer", "an amount")?;
        let data = args.get(3).map(Vec::as_slice).unwrap_or(&[]);
        let caller = engine
            .get_calling_script_hash()
            .unwrap_or_else(UInt160::zero);
        Ok(vec![u8::from(self.neo_transfer_core(
            engine, caller, &from, &to, &amount, data,
        )?)])
    }

    pub(super) fn invoke_get_gas_per_block(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        let index = LedgerContract::new()
            .current_index(&snapshot)?
            .saturating_add(1);
        Ok(self.gas_per_block_at(&snapshot, index).to_signed_bytes_le())
    }

    pub(super) fn invoke_get_register_price(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        Ok(BigInt::from(self.register_price(&snapshot)?).to_signed_bytes_le())
    }

    pub(super) fn invoke_set_register_price(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C#: validate registerPrice > 0 -> AssertCommittee -> overwrite
        // Prefix_RegisterPrice.
        let price = args
            .first()
            .map(|b| BigInt::from_signed_bytes_le(b))
            .and_then(|b| b.to_i64())
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::setRegisterPrice requires a price")
            })?;
        if price <= 0 {
            return Err(CoreError::invalid_operation(format!(
                "RegisterPrice must be positive, got {price}"
            )));
        }
        crate::committee::assert_committee(engine, "setRegisterPrice")?;
        self.put_register_price(&engine.snapshot_cache(), price)?;
        Ok(Vec::new())
    }

    pub(super) fn invoke_set_gas_per_block(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C#: validate 0 <= gasPerBlock <= 10*GAS.Factor -> AssertCommittee
        // -> write a Prefix_GasPerBlock record at (persisting index + 1).
        let gas_per_block = args
            .first()
            .map(|b| BigInt::from_signed_bytes_le(b))
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::setGasPerBlock requires a value")
            })?;
        // GAS.Factor = 10^8; the inclusive upper bound is 10 GAS.
        let max = BigInt::from(10) * BigInt::from(100_000_000i64);
        if gas_per_block < BigInt::from(0) || gas_per_block > max {
            return Err(CoreError::invalid_operation(format!(
                "GasPerBlock must be between [0, {max}]"
            )));
        }
        crate::committee::assert_committee(engine, "setGasPerBlock")?;
        // C# `engine.PersistingBlock!.Index + 1`: the method runs during
        // block persistence, so a missing persisting block is a fault
        // (matching the C# null-forgiving deref throwing on null).
        let index = engine
            .persisting_block()
            .map(|b| b.index())
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::setGasPerBlock requires a persisting block")
            })?
            .checked_add(1)
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::setGasPerBlock: block index overflow")
            })?;
        self.put_gas_per_block(&engine.snapshot_cache(), index, &gas_per_block);
        Ok(Vec::new())
    }

    pub(super) fn invoke_get_committee(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# returns ECPoint[] sorted ascending; marshaled as an Array of
        // compressed (33-byte) public-key byte strings.
        let snapshot = engine.snapshot_cache();
        Self::points_to_array_bytes(&self.committee_sorted(&snapshot)?)
    }

    pub(super) fn invoke_get_next_block_validators(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // First ValidatorsCount committee members (stored order), sorted.
        let count = usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
        let snapshot = engine.snapshot_cache();
        Self::points_to_array_bytes(&self.next_block_validators(&snapshot, count)?)
    }

    pub(super) fn invoke_get_candidates(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        // C# `GetCandidatesInternal().Select(...).Take(256).ToArray()`
        // (NeoToken.cs:528): at most the first 256 registered candidates.
        let mut candidates = self.read_registered_candidates(&snapshot)?;
        candidates.truncate(256);
        Self::candidates_to_array_bytes(&candidates)
    }

    pub(super) fn invoke_get_candidate_vote(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let pubkey_bytes = args.first().ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::getCandidateVote requires a public key")
        })?;
        // C# takes an ECPoint; an invalid key faults at marshaling.
        let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
            CoreError::invalid_operation(format!("NeoToken::getCandidateVote: bad public key: {e}"))
        })?;
        let snapshot = engine.snapshot_cache();
        Ok(self
            .candidate_vote(&snapshot, &pubkey)?
            .to_signed_bytes_le())
    }

    pub(super) fn invoke_register_candidate(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# RegisterCandidate: pre-HF_Echidna a failed witness returns
        // false WITHOUT charging the register fee (the early check is
        // skipped from Echidna because RegisterInternal repeats it); the
        // fee is charged only after that gate. RegisterInternal then
        // (re)checks the witness, creates/flips the CandidateState to
        // Registered, and emits CandidateStateChanged.
        let pubkey_bytes = args.first().ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::registerCandidate requires a public key")
        })?;
        let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
            CoreError::invalid_operation(format!(
                "NeoToken::registerCandidate: bad public key: {e}"
            ))
        })?;
        // Pre-Echidna only: a missing witness returns false before any fee.
        if !engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            let account =
                UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
            let authorized = engine.check_witness_hash(&account).map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::registerCandidate: witness: {e}"))
            })?;
            if !authorized {
                return Ok(vec![0]);
            }
        }
        // engine.AddFee(GetRegisterPrice * FeeFactor).
        let price = self.register_price(&engine.snapshot_cache())?;
        engine
            .charge_execution_fee(u64::try_from(price).unwrap_or(0))
            .map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::registerCandidate: fee: {e}"))
            })?;
        Ok(vec![u8::from(self.register_internal(
            engine,
            &pubkey,
            "registerCandidate",
        )?)])
    }

    pub(super) fn invoke_get_all_candidates(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# GetAllCandidates (NeoToken.cs:537-545): a StorageIterator
        // over the registered, non-blocked candidate entries with
        // RemovePrefix | DeserializeValues | PickField1 and prefix
        // length 1 — each element is Struct[33-byte pubkey, Votes]. The
        // 4-byte iterator id is decoded back into an InteropInterface
        // by the dispatcher.
        let results = self
            .registered_candidate_entries(&engine.snapshot_cache())?
            .into_iter()
            .map(|(_pubkey, _votes, key, item)| (key, item))
            .collect::<Vec<_>>();
        let iterator_id = engine
            .create_storage_iterator_with_options(
                results,
                1,
                FindOptions::RemovePrefix
                    | FindOptions::DeserializeValues
                    | FindOptions::PickField1,
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::getAllCandidates: {e}"))
            })?;
        Ok(iterator_id.to_le_bytes().to_vec())
    }

    pub(super) fn invoke_on_nep17_payment(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# NeoToken.OnNEP17Payment (NeoToken.cs:374-389, HF_Echidna):
        // candidate registration by paying the register price in GAS to
        // the NEO contract. The `from` argument is unused — the witness
        // requirement is RegisterInternal's, on the candidate account
        // derived from `data`'s public key.
        if engine.get_calling_script_hash() != Some(crate::GasToken::script_hash()) {
            return Err(CoreError::invalid_operation(
                "NeoToken::onNEP17Payment: only the GAS contract can call this method",
            ));
        }
        let amount = crate::args::raw_required_integer_arg(
            args,
            1,
            "NeoToken::onNEP17Payment",
            "an amount",
        )?;
        let price = self.register_price(&engine.snapshot_cache())?;
        if amount != BigInt::from(price) {
            return Err(CoreError::invalid_operation(format!(
                "NeoToken::onNEP17Payment: incorrect GAS amount; expected {price}, received {amount}"
            )));
        }
        // `data` is an Any param (it arrives BinarySerialized); C#
        // decodes its span as a secp256r1 point, faulting on anything
        // that is not a valid public key (including Null).
        let data = args.get(2).map(Vec::as_slice).unwrap_or(&[]);
        let item =
            crate::support::codec::decode_stack_value(data, "NeoToken::onNEP17Payment data")?;
        let pubkey_bytes = item.to_byte_string_bytes().ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::onNEP17Payment data: cannot convert to bytes")
        })?;
        let pubkey = ECPoint::from_bytes(&pubkey_bytes).map_err(|e| {
            CoreError::invalid_operation(format!("NeoToken::onNEP17Payment: bad public key: {e}"))
        })?;
        if !self.register_internal(engine, &pubkey, crate::NEP17_PAYMENT_METHOD)? {
            return Err(CoreError::invalid_operation(
                "NeoToken::onNEP17Payment: failed to register candidate",
            ));
        }
        // C# `await GAS.Burn(engine, Hash, amount)`: burn the GAS this
        // transfer just credited to the NEO contract's own account.
        crate::GasToken::new().gas_burn(engine, &Self::script_hash(), &amount)?;
        Ok(Vec::new())
    }

    pub(super) fn invoke_unregister_candidate(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# UnregisterCandidate: witness on the candidate account, flip the
        // CandidateState to unregistered; CheckCandidate deletes the entry
        // once it has no remaining votes.
        let pubkey_bytes = args.first().ok_or_else(|| {
            CoreError::invalid_operation("NeoToken::unregisterCandidate requires a public key")
        })?;
        let pubkey = ECPoint::from_bytes(pubkey_bytes).map_err(|e| {
            CoreError::invalid_operation(format!(
                "NeoToken::unregisterCandidate: bad public key: {e}"
            ))
        })?;
        let account =
            UInt160::from_script(&Contract::create_signature_redeem_script(pubkey.clone()));
        let authorized = engine.check_witness_hash(&account).map_err(|e| {
            CoreError::invalid_operation(format!("NeoToken::unregisterCandidate: witness: {e}"))
        })?;
        if !authorized {
            return Ok(vec![0u8]);
        }
        let snapshot = engine.snapshot_cache();
        let key = Self::candidate_key(&pubkey);
        let Some(item) = snapshot.get(&key) else {
            return Ok(vec![1u8]); // not a candidate -> true
        };
        let (registered, votes) = Self::decode_candidate_state(&item.value_bytes())?;
        if !registered {
            return Ok(vec![1u8]);
        }
        // C# `state.Registered = false; CheckCandidate(snapshot, pubkey,
        // state)` (NeoToken.cs:443,191): flip to unregistered, then when no
        // votes remain delete BOTH the candidate entry and the
        // `Prefix_VoterRewardPerCommittee` entry (otherwise a candidate that
        // accrued committee voter rewards and then lost all votes would leave
        // a stale reward record — a state-root divergence). Retain as
        // unregistered when votes remain.
        self.check_candidate(&snapshot, &pubkey, false, &votes)?;
        // C# UnregisterCandidate (NeoToken.cs:444) sends CandidateStateChanged
        // unconditionally; native SendNotification ignores AllowNotify.
        engine
            .send_notification(
                Self::script_hash(),
                NEO_CANDIDATE_STATE_CHANGED_EVENT.to_owned(),
                vec![
                    StackItem::from_byte_string(pubkey.to_bytes()),
                    StackItem::from_bool(false),
                    StackItem::from_int(votes),
                ],
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::unregisterCandidate: notify: {e}"))
            })?;
        Ok(vec![1u8])
    }

    pub(super) fn invoke_vote(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# Vote -> VoteInternal: witness on the voter, then the vote
        // transition (extracted into `vote_internal` so PolicyContract's
        // blockAccount can clear a blocked account's vote the way C#
        // calls `NEO.VoteInternal` directly).
        let account = crate::args::raw_account(args, "NeoToken::vote")?;
        // voteTo is a nullable PublicKey (bit 1 of the arg null-mask).
        let vote_to_is_null = engine
            .get_state::<NativeArgNullMask>()
            .is_some_and(|mask| mask.0 & (1 << 1) != 0);
        let vote_to: Option<ECPoint> = if vote_to_is_null {
            None
        } else {
            let bytes = args.get(1).ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::vote requires a candidate (or null)")
            })?;
            Some(ECPoint::from_bytes(bytes).map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::vote: bad candidate: {e}"))
            })?)
        };
        if !engine
            .check_witness_hash(&account)
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::vote: witness: {e}")))?
        {
            return Ok(vec![0u8]);
        }
        Ok(vec![u8::from(self.vote_internal(
            engine,
            &account,
            vote_to.as_ref(),
        )?)])
    }

    pub(super) fn invoke_get_committee_address(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        Ok(self.compute_committee_address(&snapshot)?.to_bytes())
    }

    pub(super) fn invoke_get_account_state(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let account = crate::args::raw_account(args, "NeoToken::getAccountState")?;
        let snapshot = engine.snapshot_cache();
        // C# returns the NeoAccountState struct, or null (empty payload)
        // when the account has no entry.
        Ok(self
            .read_account_state(&snapshot, &account)
            .unwrap_or_default())
    }

    pub(super) fn invoke_unclaimed_gas(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# UnclaimedGas(account, end): `end` must equal the persisting
        // block index (or Ledger.CurrentIndex + 1); compute CalculateBonus
        // for the account's NeoAccountState (zero when it has no entry).
        let account = crate::args::raw_account(args, "NeoToken::unclaimedGas")?;
        let end = args
            .get(1)
            .map(|b| BigInt::from_signed_bytes_le(b))
            .and_then(|b| b.to_u32())
            .ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::unclaimedGas requires an end index")
            })?;
        let snapshot = engine.snapshot_cache();
        let expect_end = match engine.persisting_block() {
            Some(block) => block.index(),
            None => LedgerContract::new()
                .current_index(&snapshot)?
                .saturating_add(1),
        };
        if end != expect_end {
            return Err(CoreError::invalid_operation(format!(
                "NeoToken::unclaimedGas: end {end} must equal {expect_end}"
            )));
        }
        let bonus = match self.read_account_state(&snapshot, &account) {
            Some(bytes) => {
                let state = Self::decode_neo_account_state(&bytes)?;
                self.calculate_bonus(&snapshot, &state, end)?
            }
            None => BigInt::from(0),
        };
        Ok(bonus.to_signed_bytes_le())
    }
}
