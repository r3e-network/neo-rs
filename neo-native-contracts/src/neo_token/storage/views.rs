//! Stack-value projections for NeoToken storage records.

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
    pub(in crate::neo_token) fn to_stack_value(&self) -> StackValue {
        let mut items = match crate::AccountState::new(self.balance.clone()).to_stack_value() {
            StackValue::Struct(_, items) => items,
            _ => unreachable!("AccountState always projects to Struct"),
        };
        items.push(StackValue::Integer(i64::from(self.balance_height)));
        items.push(match &self.vote_to {
            Some(pubkey) => StackValue::ByteString(pubkey.to_bytes()),
            None => StackValue::Null,
        });
        items.push(StackValue::BigInteger(
            self.last_gas_per_vote.to_signed_bytes_le(),
        ));
        StackValue::Struct(neo_vm_rs::next_stack_item_id(), items)
    }

    pub(in crate::neo_token) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let decoder = crate::support::codec::StructDecoder::new(&stack_value, "neo account state")?;
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

neo_vm::impl_interoperable_via_stack_value!(NeoAccountStateView);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::neo_token) struct CandidateState {
    pub(in crate::neo_token) registered: bool,
    pub(in crate::neo_token) votes: BigInt,
}

impl CandidateState {
    pub(in crate::neo_token) fn new(registered: bool, votes: BigInt) -> Self {
        Self { registered, votes }
    }

    pub(in crate::neo_token) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![
                StackValue::Boolean(self.registered),
                StackValue::BigInteger(self.votes.to_signed_bytes_le()),
            ],
        )
    }

    pub(in crate::neo_token) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let decoder = crate::support::codec::StructDecoder::new(&stack_value, "candidate state")?;
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

neo_vm::impl_interoperable_via_stack_value!(CandidateState);

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

    pub(crate) fn to_stack_value(&self) -> StackValue {
        StackValue::Array(
            neo_vm_rs::next_stack_item_id(),
            self.members
                .iter()
                .map(|(point, votes)| {
                    StackValue::Struct(
                        neo_vm_rs::next_stack_item_id(),
                        vec![
                            StackValue::ByteString(point.to_bytes()),
                            StackValue::BigInteger(votes.to_signed_bytes_le()),
                        ],
                    )
                })
                .collect(),
        )
    }

    pub(crate) fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Array(_, array) = stack_value else {
            return Err(CoreError::invalid_data("committee cache is not an array"));
        };
        let mut members = Vec::with_capacity(array.len());
        for element in array {
            members.push(Self::member_from_stack_value(element)?);
        }
        Ok(Self { members })
    }

    fn member_from_stack_value(stack_value: StackValue) -> CoreResult<(ECPoint, BigInt)> {
        let decoder = crate::support::codec::StructDecoder::new(&stack_value, "committee element")?;
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

neo_vm::impl_interoperable_via_stack_value!(CachedCommittee);
