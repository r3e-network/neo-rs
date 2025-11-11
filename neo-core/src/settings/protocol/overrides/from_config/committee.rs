use alloc::vec::Vec;

use neo_crypto::ecc256::PublicKey;

use crate::settings::error::ProtocolSettingsError;

pub(super) fn parse_committee(
    committee: Option<&[String]>,
) -> Result<Option<Vec<PublicKey>>, ProtocolSettingsError> {
    let Some(entries) = committee else {
        return Ok(None);
    };
    if entries.is_empty() {
        return Err(ProtocolSettingsError::EmptyCommittee);
    }
    let mut members = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            return Err(ProtocolSettingsError::EmptyCommittee);
        }
        let normalized = trimmed
            .strip_prefix("0x")
            .or_else(|| trimmed.strip_prefix("0X"))
            .unwrap_or(trimmed);
        let bytes = hex::decode(normalized)
            .map_err(|source| ProtocolSettingsError::InvalidCommitteeHex { index, source })?;
        let key = PublicKey::from_sec1_bytes(&bytes)
            .map_err(|source| ProtocolSettingsError::InvalidCommitteeKey { index, source })?;
        members.push(key);
    }
    Ok(Some(members))
}
