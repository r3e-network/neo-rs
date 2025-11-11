use serde::Deserialize;

use super::error::ProtocolSettingsError;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScryptSettings {
    pub n: u64,
    pub r: u32,
    pub p: u32,
}

impl Default for ScryptSettings {
    fn default() -> Self {
        Self {
            n: 16_384,
            r: 8,
            p: 8,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "PascalCase")]
pub struct ScryptSettingsConfig {
    pub n: Option<u64>,
    pub r: Option<u32>,
    pub p: Option<u32>,
}

impl TryFrom<ScryptSettingsConfig> for ScryptSettings {
    type Error = ProtocolSettingsError;

    fn try_from(config: ScryptSettingsConfig) -> Result<Self, Self::Error> {
        let mut settings = ScryptSettings::default();
        if let Some(n) = config.n {
            if n == 0 {
                return Err(ProtocolSettingsError::InvalidScryptParameter { param: "N" });
            }
            settings.n = n;
        }
        if let Some(r) = config.r {
            if r == 0 {
                return Err(ProtocolSettingsError::InvalidScryptParameter { param: "R" });
            }
            settings.r = r;
        }
        if let Some(p) = config.p {
            if p == 0 {
                return Err(ProtocolSettingsError::InvalidScryptParameter { param: "P" });
            }
            settings.p = p;
        }
        Ok(settings)
    }
}
