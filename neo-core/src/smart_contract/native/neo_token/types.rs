//
// types.rs - NeoAccountState and CandidateState data structures
//

use super::*;
use neo_vm_rs::StackValue;

fn stack_value_to_bigint(value: &StackValue) -> Result<BigInt, String> {
    match value {
        StackValue::Integer(value) => Ok(BigInt::from(*value)),
        StackValue::Boolean(value) => Ok(BigInt::from(i32::from(*value))),
        StackValue::BigInteger(bytes) => Ok(BigInt::from_signed_bytes_le(bytes)),
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) if bytes.len() <= 32 => {
            Ok(BigInt::from_signed_bytes_le(bytes))
        }
        _ => Err("cannot convert stack value to integer".to_string()),
    }
}

fn stack_value_to_bool(value: &StackValue) -> Result<bool, String> {
    match value {
        StackValue::Null => Ok(false),
        StackValue::Boolean(value) => Ok(*value),
        StackValue::Integer(value) => Ok(*value != 0),
        StackValue::BigInteger(bytes) => Ok(bytes.iter().any(|byte| *byte != 0)),
        StackValue::ByteString(bytes) if bytes.len() <= 32 => {
            Ok(bytes.iter().any(|byte| *byte != 0))
        }
        StackValue::ByteString(_) => {
            Err("cannot convert oversized byte string to boolean".to_string())
        }
        StackValue::Buffer(_) => Ok(true),
        StackValue::Array(_)
        | StackValue::Struct(_)
        | StackValue::Map(_)
        | StackValue::Pointer(_)
        | StackValue::Interop(_)
        | StackValue::Iterator(_) => Ok(true),
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct NeoAccountState {
    pub(crate) balance: BigInt,
    pub(crate) balance_height: u32,
    pub(crate) vote_to: Option<ECPoint>,
    pub(crate) last_gas_per_vote: BigInt,
}

impl NeoAccountState {
    fn legacy_balance(balance: BigInt) -> Self {
        Self {
            balance,
            balance_height: 0,
            vote_to: None,
            last_gas_per_vote: BigInt::zero(),
        }
    }

    pub(super) fn from_storage_item(item: &StorageItem) -> Result<Self, String> {
        let value = item.value_bytes();
        let bytes = value.as_ref();
        let stack_value = BinarySerializer::deserialize_stack_value(bytes)
            .map_err(|err| format!("failed to deserialize NeoAccountState: {}", err))?;
        Self::from_stack_value(stack_value)
    }

    pub(super) fn from_stack_value(value: StackValue) -> Result<Self, String> {
        match value {
            StackValue::Struct(entries) => {
                if entries.len() < 4 {
                    return Err("NeoAccountState struct missing fields".to_string());
                }

                let balance = stack_value_to_bigint(&entries[0])
                    .map_err(|err| format!("invalid balance: {}", err))?;

                let balance_height_big = stack_value_to_bigint(&entries[1])
                    .map_err(|err| format!("invalid balance height: {}", err))?;
                let balance_height = balance_height_big
                    .to_u32()
                    .ok_or_else(|| "balance height out of range".to_string())?;

                let vote_to = if matches!(entries[2], StackValue::Null) {
                    None
                } else {
                    let bytes = match &entries[2] {
                        StackValue::ByteString(data) | StackValue::Buffer(data) => data.clone(),
                        other => {
                            return Err(format!(
                                "vote target must be byte array, found {:?}",
                                other.compact_type_tag()
                            ))
                        }
                    };
                    Some(
                        ECPoint::from_bytes(&bytes)
                            .map_err(|err| format!("invalid vote public key: {}", err))?,
                    )
                };

                let last_gas_per_vote = stack_value_to_bigint(&entries[3])
                    .map_err(|err| format!("invalid last gas per vote: {}", err))?;

                Ok(Self {
                    balance,
                    balance_height,
                    vote_to,
                    last_gas_per_vote,
                })
            }
            StackValue::Integer(balance) => Ok(Self::legacy_balance(BigInt::from(balance))),
            StackValue::BigInteger(bytes) => {
                Ok(Self::legacy_balance(BigInt::from_signed_bytes_le(&bytes)))
            }
            StackValue::ByteString(bytes) => {
                Ok(Self::legacy_balance(BigInt::from_signed_bytes_le(&bytes)))
            }
            other => Err(format!(
                "expected NeoAccountState struct, found {:?}",
                other.compact_type_tag()
            )),
        }
    }

    pub(super) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::BigInteger(self.balance.to_signed_bytes_le()),
            StackValue::Integer(i64::from(self.balance_height)),
            match &self.vote_to {
                Some(pk) => StackValue::ByteString(pk.as_bytes().to_vec()),
                None => StackValue::Null,
            },
            StackValue::BigInteger(self.last_gas_per_vote.to_signed_bytes_le()),
        ])
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
        let value = item.value_bytes();
        let bytes = value.as_ref();
        if bytes.is_empty() {
            return Ok(Self::default());
        }

        let stack_value = BinarySerializer::deserialize_stack_value(bytes)
            .map_err(|err| format!("failed to deserialize CandidateState: {}", err))?;
        Self::from_stack_value(stack_value)
    }

    pub(super) fn from_stack_value(value: StackValue) -> Result<Self, String> {
        let entries = match value {
            StackValue::Struct(entries) | StackValue::Array(entries) => entries,
            StackValue::Integer(votes) => {
                return Ok(Self {
                    registered: votes >= 0,
                    votes: BigInt::from(votes).max(BigInt::zero()),
                })
            }
            StackValue::BigInteger(bytes) => {
                let votes = BigInt::from_signed_bytes_le(&bytes);
                let sign = votes.sign();
                return Ok(Self {
                    registered: sign != num_bigint::Sign::Minus,
                    votes: votes.max(BigInt::zero()),
                });
            }
            other => {
                return Err(format!(
                    "expected CandidateState struct, found {:?}",
                    other.compact_type_tag()
                ))
            }
        };

        if entries.len() < 2 {
            return Err("CandidateState struct missing fields".to_string());
        }

        let registered = stack_value_to_bool(&entries[0])
            .map_err(|e| format!("invalid registered field: {e}"))?;
        let votes =
            stack_value_to_bigint(&entries[1]).map_err(|e| format!("invalid votes field: {e}"))?;

        Ok(Self { registered, votes })
    }

    pub(super) fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(vec![
            StackValue::Boolean(self.registered),
            StackValue::BigInteger(self.votes.to_signed_bytes_le()),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::StackValue;

    fn sample_vote_target() -> ECPoint {
        let encoded =
            hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .expect("hex");
        ECPoint::from_bytes(&encoded).expect("valid ECPoint")
    }

    #[test]
    fn neo_account_state_projects_to_neo_vm_rs_stack_value() {
        let vote_to = sample_vote_target();
        let state = NeoAccountState {
            balance: BigInt::from(100),
            balance_height: 42,
            vote_to: Some(vote_to.clone()),
            last_gas_per_vote: BigInt::from(7),
        };

        assert_eq!(
            state.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::BigInteger(BigInt::from(100).to_signed_bytes_le()),
                StackValue::Integer(42),
                StackValue::ByteString(vote_to.as_bytes().to_vec()),
                StackValue::BigInteger(BigInt::from(7).to_signed_bytes_le()),
            ])
        );
    }

    #[test]
    fn neo_account_state_reads_from_neo_vm_rs_stack_value() {
        let vote_to = sample_vote_target();

        let state = NeoAccountState::from_stack_value(StackValue::Struct(vec![
            StackValue::BigInteger(BigInt::from(555).to_signed_bytes_le()),
            StackValue::Integer(88),
            StackValue::ByteString(vote_to.as_bytes().to_vec()),
            StackValue::BigInteger(BigInt::from(123).to_signed_bytes_le()),
        ]))
        .unwrap();

        assert_eq!(state.balance, BigInt::from(555));
        assert_eq!(state.balance_height, 88);
        assert_eq!(state.vote_to, Some(vote_to));
        assert_eq!(state.last_gas_per_vote, BigInt::from(123));
    }

    #[test]
    fn neo_account_state_reads_legacy_integer_stack_value() {
        let state = NeoAccountState::from_stack_value(StackValue::Integer(12)).unwrap();

        assert_eq!(state.balance, BigInt::from(12));
        assert_eq!(state.balance_height, 0);
        assert_eq!(state.vote_to, None);
        assert_eq!(state.last_gas_per_vote, BigInt::zero());
    }

    #[test]
    fn candidate_state_projects_to_neo_vm_rs_stack_value() {
        let state = CandidateState {
            registered: true,
            votes: BigInt::from(321),
        };

        assert_eq!(
            state.to_stack_value(),
            StackValue::Struct(vec![
                StackValue::Boolean(true),
                StackValue::BigInteger(BigInt::from(321).to_signed_bytes_le()),
            ])
        );
    }

    #[test]
    fn candidate_state_reads_from_neo_vm_rs_stack_value() {
        let state = CandidateState::from_stack_value(StackValue::Struct(vec![
            StackValue::Boolean(true),
            StackValue::BigInteger(BigInt::from(456).to_signed_bytes_le()),
        ]))
        .unwrap();

        assert!(state.registered);
        assert_eq!(state.votes, BigInt::from(456));
    }

    #[test]
    fn candidate_state_reads_legacy_integer_stack_value() {
        let state = CandidateState::from_stack_value(StackValue::Integer(-10)).unwrap();

        assert!(!state.registered);
        assert_eq!(state.votes, BigInt::zero());
    }
}
