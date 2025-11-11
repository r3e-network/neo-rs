use alloc::vec::Vec;

use hashbrown::HashSet;

use super::ConsensusState;
use crate::{message::MessageKind, validator::ValidatorId};

impl ConsensusState {
    pub fn missing_validators(&self, kind: MessageKind) -> Vec<ValidatorId> {
        let recorded = self.records.get(&kind);
        match kind {
            MessageKind::PrepareRequest => {
                let Some(primary) = self.primary() else {
                    return Vec::new();
                };
                let present = recorded
                    .map(|entries| entries.iter().any(|m| m.validator == primary))
                    .unwrap_or(false);
                if present {
                    Vec::new()
                } else {
                    vec![primary]
                }
            }
            _ => {
                let Some(expected) = self.expected_participants(kind) else {
                    return Vec::new();
                };
                let present = recorded
                    .map(|entries| entries.iter().map(|m| m.validator).collect::<HashSet<_>>())
                    .unwrap_or_default();
                let mut missing: Vec<ValidatorId> = expected
                    .into_iter()
                    .filter(|validator| !present.contains(validator))
                    .collect();
                missing.sort();
                missing
            }
        }
    }
}
