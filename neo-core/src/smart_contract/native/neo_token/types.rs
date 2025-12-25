//
// types.rs - NeoAccountState and CandidateState data structures
//

use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct NeoAccountState {
    pub(crate) balance: BigInt,
    pub(crate) balance_height: u32,
    pub(crate) vote_to: Option<ECPoint>,
    pub(crate) last_gas_per_vote: BigInt,
}

impl NeoAccountState {
    pub(super) fn from_storage_item(item: &StorageItem) -> Result<Self, String> {
        let bytes = item.get_value();
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
                .map_err(|err| format!("failed to deserialize NeoAccountState: {}", err))?;
        Self::from_stack_item(stack_item)
    }

    pub(super) fn from_stack_item(item: StackItem) -> Result<Self, String> {
        match item {
            StackItem::Struct(structure) => {
                let entries = structure.items();
                if entries.len() < 4 {
                    return Err("NeoAccountState struct missing fields".to_string());
                }

                let balance = entries[0]
                    .as_int()
                    .map_err(|err| format!("invalid balance: {}", err))?;
                let balance_height_big = entries[1]
                    .as_int()
                    .map_err(|err| format!("invalid balance height: {}", err))?;
                let balance_height = balance_height_big
                    .to_u32()
                    .ok_or_else(|| "balance height out of range".to_string())?;

                let vote_to = if entries[2].is_null() {
                    None
                } else {
                    let bytes = match &entries[2] {
                        StackItem::ByteString(data) => data.clone(),
                        StackItem::Buffer(buf) => buf.data(),
                        other => {
                            return Err(format!(
                                "vote target must be byte array, found {:?}",
                                other.stack_item_type()
                            ))
                        }
                    };
                    Some(
                        ECPoint::from_bytes(&bytes)
                            .map_err(|err| format!("invalid vote public key: {}", err))?,
                    )
                };

                let last_gas_per_vote = entries[3]
                    .as_int()
                    .map_err(|err| format!("invalid last gas per vote: {}", err))?;

                Ok(Self {
                    balance,
                    balance_height,
                    vote_to,
                    last_gas_per_vote,
                })
            }
            StackItem::Integer(balance) => Ok(Self {
                balance,
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::zero(),
            }),
            StackItem::ByteString(bytes) => Ok(Self {
                balance: BigInt::from_signed_bytes_le(&bytes),
                balance_height: 0,
                vote_to: None,
                last_gas_per_vote: BigInt::zero(),
            }),
            other => Err(format!(
                "expected NeoAccountState struct, found {:?}",
                other.stack_item_type()
            )),
        }
    }

    pub(super) fn balance(&self) -> &BigInt {
        &self.balance
    }

    pub(super) fn balance_height(&self) -> u32 {
        self.balance_height
    }

    pub(super) fn vote_to(&self) -> Option<&ECPoint> {
        self.vote_to.as_ref()
    }

    pub(super) fn last_gas_per_vote(&self) -> &BigInt {
        &self.last_gas_per_vote
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct CandidateState {
    pub(crate) registered: bool,
    pub(crate) votes: BigInt,
}

impl CandidateState {
    pub(super) fn from_storage_item(item: &StorageItem) -> Result<Self, String> {
        let bytes = item.get_value();
        if bytes.is_empty() {
            return Ok(Self::default());
        }
        let stack_item =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None)
                .map_err(|err| format!("failed to deserialize CandidateState: {}", err))?;
        Self::from_stack_item(stack_item)
    }

    pub(super) fn from_stack_item(item: StackItem) -> Result<Self, String> {
        let entries = match item {
            StackItem::Struct(structure) => structure.items(),
            StackItem::Array(array) => array.items(),
            StackItem::Integer(votes) => {
                return Ok(Self {
                    registered: votes.sign() != num_bigint::Sign::Minus,
                    votes: votes.max(BigInt::zero()),
                })
            }
            other => {
                return Err(format!(
                    "expected CandidateState struct, found {:?}",
                    other.stack_item_type()
                ))
            }
        };

        if entries.len() < 2 {
            return Err("CandidateState struct missing fields".to_string());
        }

        let registered = entries[0]
            .get_boolean()
            .map_err(|e| format!("invalid registered field: {e}"))?;
        let votes = entries[1]
            .as_int()
            .map_err(|e| format!("invalid votes field: {e}"))?;

        Ok(Self { registered, votes })
    }

    pub(super) fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_bool(self.registered),
            StackItem::from_int(self.votes.clone()),
        ])
    }
}
