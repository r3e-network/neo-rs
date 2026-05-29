use crate::network::p2p::payloads::{WitnessCondition, WitnessRule, WitnessRuleAction};
use crate::UInt160;
use neo_crypto::ECPoint;

macro_rules! impl_witness_condition_builder_methods {
    () => {
        /// Adds an AND condition.
        pub fn and<F>(&mut self, config: F) -> &mut Self
        where
            F: FnOnce(&mut AndConditionBuilder),
        {
            let mut builder = AndConditionBuilder::new();
            config(&mut builder);
            self.store_condition(builder.build())
        }

        /// Adds an OR condition.
        pub fn or<F>(&mut self, config: F) -> &mut Self
        where
            F: FnOnce(&mut OrConditionBuilder),
        {
            let mut builder = OrConditionBuilder::new();
            config(&mut builder);
            self.store_condition(builder.build())
        }

        /// Adds a boolean condition.
        pub fn boolean(&mut self, value: bool) -> &mut Self {
            self.store_condition(WitnessCondition::Boolean { value })
        }

        /// Adds a CalledByContract condition.
        pub fn called_by_contract(&mut self, hash: UInt160) -> &mut Self {
            self.store_condition(WitnessCondition::CalledByContract { hash })
        }

        /// Adds a CalledByEntry condition.
        pub fn called_by_entry(&mut self) -> &mut Self {
            self.store_condition(WitnessCondition::CalledByEntry)
        }

        /// Adds a CalledByGroup condition.
        pub fn called_by_group(&mut self, group: ECPoint) -> &mut Self {
            self.store_condition(WitnessCondition::CalledByGroup {
                group: group.as_bytes().to_vec(),
            })
        }

        /// Adds a Group condition.
        pub fn group(&mut self, group: ECPoint) -> &mut Self {
            self.store_condition(WitnessCondition::Group {
                group: group.as_bytes().to_vec(),
            })
        }

        /// Adds a ScriptHash condition.
        pub fn script_hash(&mut self, hash: UInt160) -> &mut Self {
            self.store_condition(WitnessCondition::ScriptHash { hash })
        }
    };
}

/// Builder for witness conditions.
#[must_use]
pub struct WitnessConditionBuilder {
    condition: Option<WitnessCondition>,
}

crate::impl_default_via_new!(WitnessConditionBuilder);

impl WitnessConditionBuilder {
    /// Creates a new empty witness condition builder.
    pub fn new() -> Self {
        Self { condition: None }
    }

    fn store_condition(&mut self, condition: WitnessCondition) -> &mut Self {
        self.condition = Some(condition);
        self
    }

    impl_witness_condition_builder_methods!();

    /// Adds a NOT condition wrapper.
    pub fn not<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut WitnessConditionBuilder),
    {
        let mut builder = WitnessConditionBuilder::new();
        config(&mut builder);
        self.condition = Some(WitnessCondition::Not {
            condition: Box::new(builder.build()),
        });
        self
    }

    /// Builds and returns the configured condition.
    pub fn build(&self) -> WitnessCondition {
        self.condition
            .clone()
            .unwrap_or(WitnessCondition::Boolean { value: true })
    }
}

/// Builder for witness rules.
#[must_use]
pub struct WitnessRuleBuilder {
    action: WitnessRuleAction,
    condition: Option<WitnessCondition>,
}

impl WitnessRuleBuilder {
    /// Creates a new witness rule builder with the specified action.
    pub fn new(action: WitnessRuleAction) -> Self {
        Self {
            action,
            condition: None,
        }
    }

    /// Adds a condition to the witness rule.
    pub fn add_condition<F>(&mut self, config: F) -> &mut Self
    where
        F: FnOnce(&mut WitnessConditionBuilder),
    {
        let mut builder = WitnessConditionBuilder::new();
        config(&mut builder);
        self.condition = Some(builder.build());
        self
    }

    /// Builds and returns the configured witness rule.
    pub fn build(&self) -> WitnessRule {
        WitnessRule::new(
            self.action,
            self.condition
                .clone()
                .expect("Witness rule condition must be set"),
        )
    }
}

/// Builder for `And` witness conditions.
#[must_use]
pub struct AndConditionBuilder {
    conditions: Vec<WitnessCondition>,
}

crate::impl_default_via_new!(AndConditionBuilder);

impl AndConditionBuilder {
    /// Creates a new empty AND condition builder.
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    fn store_condition(&mut self, condition: WitnessCondition) -> &mut Self {
        self.conditions.push(condition);
        self
    }

    impl_witness_condition_builder_methods!();

    /// Builds and returns the AND condition.
    pub fn build(self) -> WitnessCondition {
        WitnessCondition::And {
            conditions: self.conditions,
        }
    }
}

/// Builder for `Or` witness conditions.
#[must_use]
pub struct OrConditionBuilder {
    conditions: Vec<WitnessCondition>,
}

crate::impl_default_via_new!(OrConditionBuilder);

impl OrConditionBuilder {
    /// Creates a new empty OR condition builder.
    pub fn new() -> Self {
        Self {
            conditions: Vec::new(),
        }
    }

    fn store_condition(&mut self, condition: WitnessCondition) -> &mut Self {
        self.conditions.push(condition);
        self
    }

    impl_witness_condition_builder_methods!();

    pub fn build(self) -> WitnessCondition {
        WitnessCondition::Or {
            conditions: self.conditions,
        }
    }
}
