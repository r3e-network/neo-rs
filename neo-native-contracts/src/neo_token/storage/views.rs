//! Stack-item projections for NeoToken storage records.

use super::*;
use neo_error::CoreError;

/// Decoded view of a `NeoAccountState` (`Struct[Balance, BalanceHeight, VoteTo,
/// LastGasPerVote]`, C# `NeoAccountState.FromStackItem`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::neo_token) struct NeoAccountStateView {
    pub(in crate::neo_token) balance: BigInt,
    pub(in crate::neo_token) balance_height: u32,
    pub(in crate::neo_token) vote_to: Option<ECPoint>,
    pub(in crate::neo_token) last_gas_per_vote: BigInt,
}

impl NeoAccountStateView {
    pub(in crate::neo_token) fn to_stack_item(&self) -> StackItem {
        let mut items = match crate::AccountState::new(self.balance.clone()).to_stack_item() {
            StackItem::Struct(structure) => structure.items(),
            _ => unreachable!("AccountState always projects to Struct"),
        };
        items.push(StackItem::from_i64(i64::from(self.balance_height)));
        items.push(match &self.vote_to {
            Some(pubkey) => StackItem::from_byte_string(pubkey.to_bytes()),
            None => StackItem::Null,
        });
        items.push(StackItem::from_int(self.last_gas_per_vote.clone()));
        StackItem::from_struct(items)
    }

    pub(in crate::neo_token) fn from_stack_item(stack_item: &StackItem) -> CoreResult<Self> {
        let decoder = crate::support::codec::StructDecoder::new(stack_item, "neo account state")?;
        if decoder.len() < 4 {
            return Err(CoreError::invalid_data(
                "neo account state must have at least 4 fields",
            ));
        }

        let balance = decoder.bigint(0, "balance")?;
        let balance_height = decoder.u32(1, "balanceHeight")?;
        let vote_to = if decoder.is_null(2) {
            None
        } else {
            Some(decoder.ec_point(2, "voteTo")?)
        };
        let last_gas_per_vote = decoder.bigint(3, "lastGasPerVote")?;
        Ok(Self {
            balance,
            balance_height,
            vote_to,
            last_gas_per_vote,
        })
    }
}

neo_vm::impl_interoperable_via_stack_item!(NeoAccountStateView);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::neo_token) struct CandidateState {
    pub(in crate::neo_token) registered: bool,
    pub(in crate::neo_token) votes: BigInt,
}

impl CandidateState {
    pub(in crate::neo_token) fn new(registered: bool, votes: BigInt) -> Self {
        Self { registered, votes }
    }

    pub(in crate::neo_token) fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_bool(self.registered),
            StackItem::from_int(self.votes.clone()),
        ])
    }

    pub(in crate::neo_token) fn from_stack_item(stack_item: &StackItem) -> CoreResult<Self> {
        let decoder = crate::support::codec::StructDecoder::new(stack_item, "candidate state")?;
        if decoder.len() < 2 {
            return Err(CoreError::invalid_data(
                "candidate state must have at least 2 fields",
            ));
        }
        let registered = decoder.bool_value(0, "registered")?;
        let votes = decoder.bigint(1, "votes")?;
        Ok(Self { registered, votes })
    }
}

neo_vm::impl_interoperable_via_stack_item!(CandidateState);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CachedCommittee {
    members: Vec<(ECPoint, BigInt)>,
}

impl CachedCommittee {
    pub(crate) fn new(members: Vec<(ECPoint, BigInt)>) -> Self {
        Self { members }
    }

    pub(crate) fn into_members(self) -> Vec<(ECPoint, BigInt)> {
        self.members
    }

    pub(crate) fn to_stack_item(&self) -> StackItem {
        StackItem::from_array(
            self.members
                .iter()
                .map(|(point, votes)| {
                    StackItem::from_struct(vec![
                        StackItem::from_byte_string(point.to_bytes()),
                        StackItem::from_int(votes.clone()),
                    ])
                })
                .collect(),
        )
    }

    pub(crate) fn from_stack_item(stack_item: &StackItem) -> CoreResult<Self> {
        let StackItem::Array(array) = stack_item else {
            return Err(CoreError::invalid_data("committee cache is not an array"));
        };
        let array = array.items();
        let mut members = Vec::with_capacity(array.len());
        for element in &array {
            members.push(Self::member_from_stack_item(element)?);
        }
        Ok(Self { members })
    }

    fn member_from_stack_item(stack_item: &StackItem) -> CoreResult<(ECPoint, BigInt)> {
        let decoder = crate::support::codec::StructDecoder::new(stack_item, "committee element")?;
        if decoder.len() < 2 {
            return Err(CoreError::invalid_data(
                "committee element must have at least 2 fields",
            ));
        }
        let point = decoder.ec_point(0, "pubkey")?;
        let votes = decoder.bigint(1, "votes")?;
        Ok((point, votes))
    }
}

neo_vm::impl_interoperable_via_stack_item!(CachedCommittee);
