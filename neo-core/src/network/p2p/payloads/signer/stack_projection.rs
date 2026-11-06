use super::Signer;
use crate::witness_rule::{ToStackValue, WitnessRule};
use neo_primitives::WitnessScope;
use neo_vm::{StackItem, StackValue, VmError};

/// Converts the signer to a neo-vm-rs stack value (matches C# `Signer.ToStackItem` layout).
pub fn signer_to_stack_value(signer: &Signer) -> StackValue {
    let allowed_contracts = if signer.scopes.contains(WitnessScope::CUSTOM_CONTRACTS) {
        signer
            .allowed_contracts
            .iter()
            .copied()
            .map(|hash| StackValue::ByteString(hash.to_bytes()))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let allowed_groups = if signer.scopes.contains(WitnessScope::CUSTOM_GROUPS) {
        signer
            .allowed_groups
            .iter()
            .map(|group| StackValue::ByteString(group.to_bytes()))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let rules = if signer.scopes.contains(WitnessScope::WITNESS_RULES) {
        signer
            .rules
            .iter()
            .map(WitnessRule::to_stack_value)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    StackValue::Array(vec![
        StackValue::ByteString(signer.account.to_bytes()),
        StackValue::Integer(i64::from(signer.scopes.bits())),
        StackValue::Array(allowed_contracts),
        StackValue::Array(allowed_groups),
        StackValue::Array(rules),
    ])
}

/// Converts the signer projection to a VM stack item.
pub fn signer_to_stack_item(signer: &Signer) -> Result<StackItem, VmError> {
    StackItem::try_from(signer_to_stack_value(signer)).map_err(|error| {
        VmError::invalid_operation_msg(format!(
            "Failed to convert signer StackValue to StackItem: {error}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::witness_rule::{WitnessCondition, WitnessRuleAction};
    use neo_primitives::{UINT160_SIZE, UInt160};

    #[test]
    fn signer_projects_to_neo_vm_rs_stack_value() {
        let account = UInt160::from_bytes(&[0x11; UINT160_SIZE]).unwrap();
        let allowed_contract = UInt160::from_bytes(&[0x22; UINT160_SIZE]).unwrap();
        let rule = WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::Boolean { value: true },
        );
        let scopes = WitnessScope::CUSTOM_CONTRACTS | WitnessScope::WITNESS_RULES;
        let mut signer = Signer::new(account, scopes);
        signer.allowed_contracts.push(allowed_contract);
        signer.rules.push(rule.clone());

        assert_eq!(
            signer_to_stack_value(&signer),
            StackValue::Array(vec![
                StackValue::ByteString(account.to_bytes()),
                StackValue::Integer(i64::from(scopes.bits())),
                StackValue::Array(vec![StackValue::ByteString(allowed_contract.to_bytes())]),
                StackValue::Array(Vec::new()),
                StackValue::Array(vec![rule.to_stack_value()]),
            ])
        );
    }

    #[test]
    fn signer_stack_item_projection_matches_stack_value_projection() {
        let account = UInt160::from_bytes(&[0x33; UINT160_SIZE]).unwrap();
        let signer = Signer::new(account, WitnessScope::CALLED_BY_ENTRY);

        let expected = StackItem::try_from(signer_to_stack_value(&signer)).unwrap();
        assert_eq!(signer_to_stack_item(&signer).unwrap(), expected);
    }
}
