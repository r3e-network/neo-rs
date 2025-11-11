use neo_base::hash::Hash160;
use neo_core::tx::{WitnessConditionContext, WitnessScope};

use super::ExecutionContext;

impl<'a> ExecutionContext<'a> {
    pub fn check_witness(&self, target: &Hash160) -> bool {
        if let Some(legacy) = self.legacy_signer {
            if &legacy == target {
                return true;
            }
        }

        if self.signers.is_empty() {
            return false;
        }

        let target_bytes: &[u8] = target.as_slice();
        let context = self.witness_condition_context();

        for signer in &self.signers {
            if signer.account.as_le_bytes() != target_bytes {
                continue;
            }
            if signer.scopes.has_scope(WitnessScope::Global) {
                return true;
            }
            if let Some(ctx) = context.as_ref() {
                if signer.allows(ctx) {
                    return true;
                }
            }
        }

        false
    }

    fn witness_condition_context(&self) -> Option<WitnessConditionContext<'_>> {
        let current = self.current_script_hash?;
        let mut ctx = WitnessConditionContext::new(current);
        if let Some(entry) = self.entry_script_hash {
            if entry == current {
                ctx = ctx.called_by_entry(true);
            }
        }
        if let Some(calling) = self.calling_script_hash {
            ctx = ctx.with_calling_script(calling);
        }
        if !self.current_contract_groups.is_empty() {
            ctx = ctx.with_current_groups(&self.current_contract_groups);
        }
        if !self.calling_contract_groups.is_empty() {
            ctx = ctx.with_calling_groups(&self.calling_contract_groups);
        }
        Some(ctx)
    }
}
