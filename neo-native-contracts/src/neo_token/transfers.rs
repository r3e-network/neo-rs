use super::*;

impl NeoToken {
    /// C# `NeoToken.OnBalanceChanging`: invoked whenever an account's NEO balance is
    /// about to change by `amount` (a signed delta). It (a) computes the account's
    /// accrued GAS via `DistributeGas` — mutating `state.balance_height` /
    /// `state.last_gas_per_vote` and returning the datoshi to mint (or `None`), and
    /// (b) when the account votes, shifts that candidate's vote weight and the global
    /// voters-count by `amount`. The caller writes `state` back and mints the return.
    pub(super) fn neo_on_balance_changing(
        &self,
        engine: &ApplicationEngine,
        snapshot: &DataCache,
        state: &mut NeoAccountStateView,
        amount: &BigInt,
    ) -> CoreResult<Option<BigInt>> {
        // DistributeGas: bonus on the OLD state, then advance the reward markers.
        let mut distribution = None;
        if let Some(block) = engine.persisting_block() {
            let end = block.index();
            let bonus = self.calculate_bonus(snapshot, state, end)?;
            state.balance_height = end;
            if let Some(vote_to) = &state.vote_to {
                state.last_gas_per_vote = self.voter_reward_per_committee(snapshot, vote_to);
            }
            if bonus != BigInt::from(0) {
                distribution = Some(bonus);
            }
        }
        // Vote-weight: a balance delta moves the voted candidate's weight + voters count.
        if *amount != BigInt::from(0) {
            if let Some(vote_to) = state.vote_to.clone() {
                let mut count = self.read_voters_count(snapshot);
                count += amount;
                self.write_voters_count(snapshot, &count);
                if let Some(item) = snapshot.get(&Self::candidate_key(&vote_to)) {
                    let (registered, mut votes) =
                        Self::decode_candidate_state(&item.value_bytes())?;
                    votes += amount;
                    self.check_candidate(snapshot, &vote_to, registered, &votes)?;
                }
            }
        }
        Ok(distribution)
    }

    /// C# `FungibleToken.PostTransferAsync` for NEO: emit `Transfer(from, to, amount)`
    /// and, when `to` is a deployed contract, run its `onNEP17Payment` callback
    /// before deferred GAS distributions are minted.
    pub(super) fn neo_post_transfer(
        &self,
        engine: &mut ApplicationEngine,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: &[u8],
    ) -> CoreResult<()> {
        engine
            .send_notification(
                NeoToken::script_hash(),
                crate::NEP17_TRANSFER_EVENT.to_owned(),
                crate::nep17_transfer_notification_state(Some(from), Some(to), amount),
            )
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::transfer notify: {e}")))?;
        if !crate::ContractManagement::is_contract(&engine.snapshot_cache(), to) {
            return Ok(());
        }
        let data_item = crate::nep17_payment_data_item(data, "NeoToken::transfer data")?;
        let neo_hash = NeoToken::script_hash();
        engine.call_from_native_contract_void(
            &neo_hash,
            to,
            crate::NEP17_PAYMENT_METHOD,
            crate::nep17_payment_callback_args(Some(from), amount, data_item),
        )?;
        Ok(())
    }

    /// C# `FungibleToken.Transfer` specialised to NEO (`NeoAccountState`): witness the
    /// `from` account (with the calling-contract bypass), move the balance applying
    /// `OnBalanceChanging` on each side, then `PostTransfer` and mint the collected
    /// GAS distributions. Returns `false` (no fault) on a failed witness / missing
    /// source / insufficient balance, matching C#.
    pub(super) fn neo_transfer_core(
        &self,
        engine: &mut ApplicationEngine,
        caller: UInt160,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: &[u8],
    ) -> CoreResult<bool> {
        if *amount < BigInt::from(0) {
            return Err(CoreError::invalid_operation(
                "NeoToken::transfer: amount cannot be negative",
            ));
        }
        if caller != *from
            && !engine.check_witness(from).map_err(|e| {
                CoreError::invalid_operation(format!("NeoToken::transfer: witness: {e}"))
            })?
        {
            return Ok(false);
        }
        let snapshot = engine.snapshot_cache();
        let zero = BigInt::from(0);
        let mut distributions: Vec<(UInt160, BigInt)> = Vec::new();
        let from_state = self.read_account_state(&snapshot, from);

        if *amount == zero {
            if let Some(bytes) = from_state {
                let mut state = Self::decode_neo_account_state(&bytes)?;
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut state, &zero)?
                {
                    distributions.push((*from, d));
                }
                snapshot.update(
                    Self::account_key(from),
                    StorageItem::from_bytes(Self::encode_neo_account_state(&state)?),
                );
            }
        } else {
            let Some(bytes) = from_state else {
                return Ok(false);
            };
            let mut from_state = Self::decode_neo_account_state(&bytes)?;
            if from_state.balance < *amount {
                return Ok(false);
            }
            if from == to {
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut from_state, &zero)?
                {
                    distributions.push((*from, d));
                }
                snapshot.update(
                    Self::account_key(from),
                    StorageItem::from_bytes(Self::encode_neo_account_state(&from_state)?),
                );
            } else {
                let neg_amount = -amount;
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut from_state, &neg_amount)?
                {
                    distributions.push((*from, d));
                }
                if from_state.balance == *amount {
                    snapshot.delete(&Self::account_key(from));
                } else {
                    from_state.balance -= amount;
                    snapshot.update(
                        Self::account_key(from),
                        StorageItem::from_bytes(Self::encode_neo_account_state(&from_state)?),
                    );
                }
                let mut to_state = match self.read_account_state(&snapshot, to) {
                    Some(bytes) => Self::decode_neo_account_state(&bytes)?,
                    None => NeoAccountStateView {
                        balance: BigInt::from(0),
                        balance_height: 0,
                        vote_to: None,
                        last_gas_per_vote: BigInt::from(0),
                    },
                };
                if let Some(d) =
                    self.neo_on_balance_changing(engine, &snapshot, &mut to_state, amount)?
                {
                    distributions.push((*to, d));
                }
                to_state.balance += amount;
                snapshot.update(
                    Self::account_key(to),
                    StorageItem::from_bytes(Self::encode_neo_account_state(&to_state)?),
                );
            }
        }

        self.neo_post_transfer(engine, from, to, amount, data)?;
        for (account, datoshi) in distributions {
            crate::GasToken::new().gas_mint(engine, &account, &datoshi, true)?;
        }
        Ok(true)
    }

    /// C# `NeoToken.VoteInternal(engine, account, voteTo)`: the vote transition
    /// applied after the caller has authorized the voter — `_votersCount`
    /// bookkeeping, the GAS reward (`DistributeGas` + `GAS.Mint`), candidate
    /// vote-weight deltas, the `NeoAccountState.VoteTo` update, and the `Vote`
    /// notification. Returns `false` (no fault) when the account has no state, a
    /// zero balance, or the new candidate is missing/unregistered, matching C#.
    ///
    /// Exposed `pub(crate)` because C# `PolicyContract.BlockAccountInternal`
    /// (HF_Faun) clears a blocked account's vote by calling
    /// `NEO.VoteInternal(engine, account, null)` directly, bypassing the witness
    /// check performed by the public `vote` method.
    pub(crate) fn vote_internal(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        vote_to: Option<&ECPoint>,
    ) -> CoreResult<bool> {
        let vote_to: Option<ECPoint> = vote_to.cloned();
        let snapshot = engine.snapshot_cache();
        let Some(acct_bytes) = self.read_account_state(&snapshot, account) else {
            return Ok(false); // no account state
        };
        let mut acct = Self::decode_neo_account_state(&acct_bytes)?;
        if acct.balance == BigInt::from(0) {
            return Ok(false);
        }
        // The new candidate must exist and be registered.
        if let Some(new_pk) = &vote_to {
            match snapshot.get(&Self::candidate_key(new_pk)) {
                Some(item) => {
                    let (registered, _) = Self::decode_candidate_state(&item.value_bytes())?;
                    if !registered {
                        return Ok(false);
                    }
                }
                None => return Ok(false),
            }
        }
        let old_vote = acct.vote_to.clone();
        // _votersCount changes only when the account starts or stops voting.
        if old_vote.is_none() != vote_to.is_none() {
            let mut count = self.read_voters_count(&snapshot);
            if old_vote.is_none() {
                count += &acct.balance;
            } else {
                count -= &acct.balance;
            }
            self.write_voters_count(&snapshot, &count);
        }
        // DistributeGas: compute the bonus with the OLD state, then advance
        // BalanceHeight + LastGasPerVote (only when a persisting block exists).
        let mut gas_to_mint = BigInt::from(0);
        if let Some(block) = engine.persisting_block() {
            let end = block.index();
            let bonus = self.calculate_bonus(&snapshot, &acct, end)?;
            acct.balance_height = end;
            if let Some(old_pk) = &old_vote {
                acct.last_gas_per_vote = self.voter_reward_per_committee(&snapshot, old_pk);
            }
            if bonus != BigInt::from(0) {
                gas_to_mint = bonus;
            }
        }
        // Remove the account's weight from the previously-voted candidate.
        if let Some(old_pk) = &old_vote {
            if let Some(item) = snapshot.get(&Self::candidate_key(old_pk)) {
                let (registered, mut votes) = Self::decode_candidate_state(&item.value_bytes())?;
                votes -= &acct.balance;
                self.check_candidate(&snapshot, old_pk, registered, &votes)?;
            }
        }
        // Switching to a new (different) candidate resets the reward marker.
        if let Some(new_pk) = &vote_to {
            if Some(new_pk) != old_vote.as_ref() {
                acct.last_gas_per_vote = self.voter_reward_per_committee(&snapshot, new_pk);
            }
        }
        let from = old_vote.clone();
        acct.vote_to = vote_to.clone();
        // Add the account's weight to the new candidate (re-read so a vote
        // for the same candidate nets to zero), else clear the reward marker.
        if let Some(new_pk) = &vote_to {
            let item = snapshot.get(&Self::candidate_key(new_pk)).ok_or_else(|| {
                CoreError::invalid_operation("NeoToken::vote: candidate disappeared")
            })?;
            let (registered, mut votes) = Self::decode_candidate_state(&item.value_bytes())?;
            votes += &acct.balance;
            snapshot.update(
                Self::candidate_key(new_pk),
                StorageItem::from_bytes(Self::encode_candidate_state(registered, &votes)?),
            );
        } else {
            acct.last_gas_per_vote = BigInt::from(0);
        }
        snapshot.update(
            Self::account_key(account),
            StorageItem::from_bytes(Self::encode_neo_account_state(&acct)?),
        );

        let to_item = |pk: &Option<ECPoint>| match pk {
            Some(p) => StackItem::from_byte_string(p.to_bytes()),
            None => StackItem::null(),
        };
        engine
            .send_notification(
                NeoToken::script_hash(),
                NEO_VOTE_EVENT.to_owned(),
                vec![
                    StackItem::from_byte_string(account.to_bytes()),
                    to_item(&from),
                    to_item(&vote_to),
                    StackItem::from_int(acct.balance.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::vote: notify: {e}")))?;
        if gas_to_mint > BigInt::from(0) {
            crate::GasToken::new().gas_mint(engine, account, &gas_to_mint, true)?;
        }
        Ok(true)
    }

    /// C# `FungibleToken.Mint` specialised to NEO (`NeoAccountState` +
    /// `OnBalanceChanging` + the GAS-distribution drain of NEO's
    /// `PostTransferAsync`): credit `amount` NEO to `account`, raise the stored
    /// total supply, emit `Transfer(null, account, amount)`, run the recipient's
    /// `onNEP17Payment` when `call_on_payment` and the recipient is a deployed
    /// contract, then mint any GAS distribution collected by `OnBalanceChanging`.
    /// A zero amount is a no-op; a negative amount faults.
    pub(super) fn neo_mint(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> CoreResult<()> {
        let zero = BigInt::from(0);
        if *amount < zero {
            return Err(CoreError::invalid_operation(
                "NeoToken::mint: amount cannot be negative",
            ));
        }
        if *amount == zero {
            return Ok(());
        }
        let snapshot = engine.snapshot_cache();
        let mut state = match self.read_account_state(&snapshot, account) {
            Some(bytes) => Self::decode_neo_account_state(&bytes)?,
            None => NeoAccountStateView {
                balance: BigInt::from(0),
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::from(0),
            },
        };
        let mut distributions: Vec<(UInt160, BigInt)> = Vec::new();
        if let Some(datoshi) =
            self.neo_on_balance_changing(engine, &snapshot, &mut state, amount)?
        {
            distributions.push((*account, datoshi));
        }
        state.balance += amount;
        snapshot.update(
            Self::account_key(account),
            StorageItem::from_bytes(Self::encode_neo_account_state(&state)?),
        );
        let supply_key = Self::total_supply_key();
        let supply = snapshot
            .get(&supply_key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(|| BigInt::from(0))
            + amount;
        snapshot.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
        // PostTransfer with from = null (C# PostTransferAsync(null, account, …)).
        engine
            .send_notification(
                NeoToken::script_hash(),
                crate::NEP17_TRANSFER_EVENT.to_owned(),
                crate::nep17_transfer_notification_state(None, Some(account), amount),
            )
            .map_err(|e| CoreError::invalid_operation(format!("NeoToken::mint notify: {e}")))?;
        if call_on_payment
            && crate::ContractManagement::is_contract(&engine.snapshot_cache(), account)
        {
            let neo_hash = NeoToken::script_hash();
            engine.call_from_native_contract_void(
                &neo_hash,
                account,
                crate::NEP17_PAYMENT_METHOD,
                crate::nep17_payment_callback_args(None, amount, StackItem::null()),
            )?;
        }
        for (target, datoshi) in distributions {
            crate::GasToken::new().gas_mint(engine, &target, &datoshi, call_on_payment)?;
        }
        Ok(())
    }
}
