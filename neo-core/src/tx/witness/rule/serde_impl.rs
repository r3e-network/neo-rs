use serde::{de::Error as DeError, Deserialize, Deserializer, Serialize, Serializer};

use crate::tx::condition::{WitnessCondition, WitnessConditionDto};

use super::{Action, WitnessRule};

#[derive(Deserialize, Serialize)]
struct WitnessRuleSerde {
    action: Action,
    #[serde(flatten)]
    condition: WitnessConditionDto,
}

impl Serialize for WitnessRule {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let helper = WitnessRuleSerde {
            action: self.action,
            condition: WitnessConditionDto::from(&self.condition),
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WitnessRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let helper = WitnessRuleSerde::deserialize(deserializer)?;
        let condition = WitnessCondition::try_from(helper.condition)
            .map_err(|err| DeError::custom(err.to_string()))?;
        Ok(Self {
            action: helper.action,
            condition,
        })
    }
}
