use super::witness_rule_to_stack_value;
use neo_primitives::WitnessScope;
use neo_protocol::payloads::Signer;
use neo_vm::{Interoperable, StackItem, StackValue, VmError};

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
            .map(witness_rule_to_stack_value)
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

/// An owned signer adapter for VM interoperability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerInterop(pub Signer);

impl From<Signer> for SignerInterop {
    fn from(signer: Signer) -> Self {
        Self(signer)
    }
}

impl Interoperable for SignerInterop {
    fn from_stack_item(&mut self, _stack_item: StackItem) -> Result<(), VmError> {
        // This operation is not supported for Signer.
        // The C# implementation throws NotSupportedException.
        Err(VmError::invalid_operation_msg(
            "Signer::from_stack_item is not supported",
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, VmError> {
        signer_to_stack_item(&self.0)
    }

    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::ECPoint;
    use neo_io::witness_rule::{WitnessCondition, WitnessRule, WitnessRuleAction};
    use neo_primitives::{UINT160_SIZE, UInt160};

    fn allowed_group() -> ECPoint {
        ECPoint::from_bytes(&[
            0x03, 0x6b, 0x17, 0xd1, 0xf2, 0xe1, 0x2c, 0x42, 0x47, 0xf8, 0xbc, 0xe6, 0xe5, 0x63,
            0xa4, 0x40, 0xf2, 0x77, 0x03, 0x7d, 0x81, 0x2d, 0xeb, 0x33, 0xa0, 0xf4, 0xa1, 0x39,
            0x45, 0xd8, 0x98, 0xc2, 0x96,
        ])
        .unwrap()
    }

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
                StackValue::Array(vec![witness_rule_to_stack_value(&rule)]),
            ])
        );
    }

    #[test]
    fn signer_stack_item_projection_matches_stack_value_projection() {
        let account = UInt160::from_bytes(&[0x33; UINT160_SIZE]).unwrap();
        let signer = Signer::new(account, WitnessScope::CALLED_BY_ENTRY);

        let expected = StackItem::try_from(signer_to_stack_value(&signer)).unwrap();
        assert_eq!(signer_to_stack_item(&signer).unwrap(), expected);
        assert_eq!(SignerInterop(signer).to_stack_item().unwrap(), expected);
    }

    #[test]
    fn signer_projection_preserves_custom_groups_and_all_five_fields() {
        let account = UInt160::from_bytes(&[0x44; UINT160_SIZE]).unwrap();
        let contract = UInt160::from_bytes(&[0x55; UINT160_SIZE]).unwrap();
        let group = allowed_group();
        let rule = WitnessRule::new(WitnessRuleAction::Deny, WitnessCondition::CalledByEntry);
        let scopes = WitnessScope::CUSTOM_CONTRACTS
            | WitnessScope::CUSTOM_GROUPS
            | WitnessScope::WITNESS_RULES;
        let mut signer = Signer::new(account, scopes);
        signer.allowed_contracts.push(contract);
        signer.allowed_groups.push(group.clone());
        signer.rules.push(rule.clone());
        assert_eq!(
            signer_to_stack_value(&signer),
            StackValue::Array(vec![
                StackValue::ByteString(account.to_bytes()),
                StackValue::Integer(i64::from(scopes.bits())),
                StackValue::Array(vec![StackValue::ByteString(contract.to_bytes())]),
                StackValue::Array(vec![StackValue::ByteString(group.to_bytes())]),
                StackValue::Array(vec![witness_rule_to_stack_value(&rule)]),
            ])
        );
        let expected = StackItem::try_from(signer_to_stack_value(&signer)).unwrap();
        assert_eq!(signer_to_stack_item(&signer).unwrap(), expected);
    }

    #[test]
    fn signer_projection_omits_unscoped_collections() {
        let account = UInt160::from_bytes(&[0x66; UINT160_SIZE]).unwrap();
        let mut signer = Signer::new(account, WitnessScope::NONE);
        signer.allowed_contracts.push(UInt160::zero());
        signer.allowed_groups.push(allowed_group());
        signer.rules.push(WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::CalledByEntry,
        ));
        for scopes in [
            WitnessScope::NONE,
            WitnessScope::CALLED_BY_ENTRY,
            WitnessScope::GLOBAL,
        ] {
            signer.scopes = scopes;
            assert_eq!(
                signer_to_stack_value(&signer),
                StackValue::Array(vec![
                    StackValue::ByteString(account.to_bytes()),
                    StackValue::Integer(i64::from(scopes.bits())),
                    StackValue::Array(Vec::new()),
                    StackValue::Array(Vec::new()),
                    StackValue::Array(Vec::new()),
                ])
            );
        }
    }

    #[test]
    fn signer_interop_clone_box_owns_an_independent_signer() {
        let (boxed, expected) = {
            let mut interop = SignerInterop(Signer::new(UInt160::zero(), WitnessScope::NONE));
            let expected = interop.to_stack_item().unwrap();
            let boxed = interop.clone_box();
            interop.0.account = UInt160::from_bytes(&[0x77; UINT160_SIZE]).unwrap();
            assert_ne!(interop.to_stack_item().unwrap(), expected);
            (boxed, expected)
        };
        assert_eq!(boxed.to_stack_item().unwrap(), expected);
        assert_eq!(boxed.clone_box().to_stack_item().unwrap(), expected);
    }

    #[test]
    fn signer_interop_from_stack_item_preserves_unsupported_message() {
        let signer = Signer::new(UInt160::zero(), WitnessScope::NONE);
        let mut interop = SignerInterop(signer.clone());
        assert_eq!(
            interop.from_stack_item(StackItem::null()).unwrap_err(),
            VmError::invalid_operation_msg("Signer::from_stack_item is not supported")
        );
        assert_eq!(interop.0, signer);
    }
}
