use crate::settings::{error::ProtocolSettingsError, ProtocolSettings};
use neo_crypto::ecc256::PublicKey;

pub(super) fn apply_committee_overrides(
    settings: &mut ProtocolSettings,
    committee_override: Option<Vec<PublicKey>>,
    validators_override: Option<usize>,
) -> Result<(), ProtocolSettingsError> {
    if let Some(committee) = committee_override {
        if committee.is_empty() {
            return Err(ProtocolSettingsError::EmptyCommittee);
        }
        settings.standby_committee = committee;
        if settings.validators_count > settings.standby_committee.len() {
            settings.validators_count = settings.standby_committee.len();
        }
    }

    if let Some(validators) = validators_override {
        if validators > settings.standby_committee.len() {
            return Err(ProtocolSettingsError::ValidatorsExceedCommittee {
                requested: validators,
                available: settings.standby_committee.len(),
            });
        }
        settings.validators_count = validators;
    }

    Ok(())
}
