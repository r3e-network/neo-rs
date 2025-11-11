use super::super::Signer;
use crate::tx::WitnessConditionContext;

impl Signer {
    pub fn allows(&self, ctx: &WitnessConditionContext<'_>) -> bool {
        for rule in self.all_rules() {
            if let Some(decision) = rule.evaluate(ctx) {
                return decision;
            }
        }
        false
    }
}
