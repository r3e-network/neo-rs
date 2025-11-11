use hashbrown::HashMap;

use crate::{
    error::ConsensusError,
    message::MessageKind,
    validator::{ValidatorId, ValidatorSet},
};

pub(super) fn restore_expected_validators(
    validators: &ValidatorSet,
    expected: HashMap<MessageKind, Vec<ValidatorId>>,
) -> Result<HashMap<MessageKind, Vec<ValidatorId>>, ConsensusError> {
    let mut expected_map = HashMap::new();
    for (kind, validators_list) in expected {
        for validator in &validators_list {
            if validators.get(*validator).is_none() {
                return Err(ConsensusError::UnknownValidator(*validator));
            }
        }
        expected_map.insert(kind, validators_list);
    }
    Ok(expected_map)
}
