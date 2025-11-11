use alloc::{format, string::String};
use core::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum WitnessScope {
    None = 0x00,
    CalledByEntry = 0x01,
    CustomContracts = 0x10,
    CustomGroups = 0x20,
    WitnessRules = 0x40,
    Global = 0x80,
}

impl fmt::Display for WitnessScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            WitnessScope::None => "None",
            WitnessScope::CalledByEntry => "CalledByEntry",
            WitnessScope::CustomContracts => "CustomContracts",
            WitnessScope::CustomGroups => "CustomGroups",
            WitnessScope::WitnessRules => "WitnessRules",
            WitnessScope::Global => "Global",
        };
        f.write_str(name)
    }
}

impl FromStr for WitnessScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            v if v.eq_ignore_ascii_case("None") => Ok(WitnessScope::None),
            v if v.eq_ignore_ascii_case("CalledByEntry") => Ok(WitnessScope::CalledByEntry),
            v if v.eq_ignore_ascii_case("CustomContracts") => Ok(WitnessScope::CustomContracts),
            v if v.eq_ignore_ascii_case("CustomGroups") => Ok(WitnessScope::CustomGroups),
            v if v.eq_ignore_ascii_case("WitnessRules") => Ok(WitnessScope::WitnessRules),
            v if v.eq_ignore_ascii_case("Global") => Ok(WitnessScope::Global),
            other => Err(format!("Unknown witness scope '{other}'")),
        }
    }
}
