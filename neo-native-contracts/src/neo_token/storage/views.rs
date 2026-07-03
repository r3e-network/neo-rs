//! Stack-value projections for NeoToken storage records.

use super::*;

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
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data("neo account state is not a struct"));
        };
        if items.len() < 4 {
            return Err(CoreError::invalid_data(
                "neo account state must have at least 4 fields",
            ));
        }

        let balance = crate::AccountState::from_stack_value(StackValue::Struct(
            neo_vm_rs::next_stack_item_id(),
            vec![items[0].clone()],
        ))?
        .balance;
        let balance_height = neo_vm_rs::stack_value_as_u32(&items[1]).ok_or_else(|| {
            CoreError::invalid_data("account balanceHeight: expected uint32 integer")
        })?;
        let vote_to = if matches!(items[2], StackValue::Null) {
            None
        } else {
            let bytes = items[2].to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_data("account voteTo: expected byte-compatible value")
            })?;
            Some(
                ECPoint::from_bytes(&bytes)
                    .map_err(|e| CoreError::invalid_data(format!("account voteTo point: {e}")))?,
            )
        };
        let last_gas_per_vote = neo_vm::stack_value_as_bigint(&items[3])
            .map_err(|e| CoreError::invalid_data(format!("account lastGasPerVote: {e}")))?;
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
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data("candidate state is not a struct"));
        };
        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "candidate state must have at least 2 fields",
            ));
        }

        let registered = neo_vm_rs::stack_value_as_bool(&items[0]).ok_or_else(|| {
            CoreError::invalid_data("candidate registered: expected boolean-compatible value")
        })?;
        let votes = neo_vm::stack_value_as_bigint(&items[1])
            .map_err(|e| CoreError::invalid_data(format!("candidate votes: {e}")))?;
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
        let StackValue::Struct(_, items) = stack_value else {
            return Err(CoreError::invalid_data("committee element is not a struct"));
        };
        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "committee element must have at least 2 fields",
            ));
        }
        let bytes = items[0]
            .to_byte_string_bytes()
            .ok_or_else(|| CoreError::invalid_data("committee pubkey: not bytes"))?;
        let point = ECPoint::from_bytes(&bytes)
            .map_err(|e| CoreError::invalid_data(format!("committee EC point: {e}")))?;
        let votes = neo_vm::stack_value_as_bigint(&items[1])
            .map_err(|e| CoreError::invalid_data(format!("committee votes: {e}")))?;
        Ok((point, votes))
    }
}

neo_vm::impl_interoperable_via_stack_value!(CachedCommittee);
