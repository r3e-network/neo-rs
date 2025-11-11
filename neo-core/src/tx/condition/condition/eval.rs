use neo_crypto::ecc256::PublicKey;

use super::{WitnessCondition, WitnessConditionContext};

impl WitnessCondition {
    pub fn matches(&self, ctx: &WitnessConditionContext<'_>) -> bool {
        match self {
            WitnessCondition::Boolean { expression } => *expression,
            WitnessCondition::Not { expression } => !expression.matches(ctx),
            WitnessCondition::And { expressions } => {
                expressions.iter().all(|condition| condition.matches(ctx))
            }
            WitnessCondition::Or { expressions } => {
                expressions.iter().any(|condition| condition.matches(ctx))
            }
            WitnessCondition::ScriptHash { hash } => ctx.current_script_hash == *hash,
            WitnessCondition::Group { group } => {
                group_in_manifest(ctx.current_contract_groups, group)
            }
            WitnessCondition::CalledByEntry => ctx.is_called_by_entry,
            WitnessCondition::CalledByContract { hash } => ctx
                .calling_script_hash
                .map(|value| value == *hash)
                .unwrap_or(false),
            WitnessCondition::CalledByGroup { group } => {
                group_in_manifest(ctx.calling_contract_groups, group)
            }
        }
    }
}

fn group_in_manifest(groups: Option<&[PublicKey]>, group: &PublicKey) -> bool {
    groups
        .map(|entries| entries.iter().any(|entry| entry == group))
        .unwrap_or(false)
}
