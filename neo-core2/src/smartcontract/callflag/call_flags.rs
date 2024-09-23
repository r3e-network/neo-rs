use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CallFlag(u8);

impl CallFlag {
    pub const READ_STATES: CallFlag = CallFlag(1 << 0);
    pub const WRITE_STATES: CallFlag = CallFlag(1 << 1);
    pub const ALLOW_CALL: CallFlag = CallFlag(1 << 2);
    pub const ALLOW_NOTIFY: CallFlag = CallFlag(1 << 3);

    pub const STATES: CallFlag = CallFlag(Self::READ_STATES.0 | Self::WRITE_STATES.0);
    pub const READ_ONLY: CallFlag = CallFlag(Self::READ_STATES.0 | Self::ALLOW_CALL.0);
    pub const ALL: CallFlag = CallFlag(Self::STATES.0 | Self::ALLOW_CALL.0 | Self::ALLOW_NOTIFY.0);
    pub const NONE: CallFlag = CallFlag(0);

    pub fn has(&self, cf: CallFlag) -> bool {
        self.0 & cf.0 == cf.0
    }
}

lazy_static! {
    static ref FLAG_STRING: HashMap<CallFlag, &'static str> = {
        let mut m = HashMap::new();
        m.insert(CallFlag::READ_STATES, "ReadStates");
        m.insert(CallFlag::WRITE_STATES, "WriteStates");
        m.insert(CallFlag::ALLOW_CALL, "AllowCall");
        m.insert(CallFlag::ALLOW_NOTIFY, "AllowNotify");
        m.insert(CallFlag::STATES, "States");
        m.insert(CallFlag::READ_ONLY, "ReadOnly");
        m.insert(CallFlag::ALL, "All");
        m.insert(CallFlag::NONE, "None");
        m
    };

    static ref BASIC_FLAGS: Vec<CallFlag> = vec![
        CallFlag::READ_ONLY,
        CallFlag::STATES,
        CallFlag::READ_STATES,
        CallFlag::WRITE_STATES,
        CallFlag::ALLOW_CALL,
        CallFlag::ALLOW_NOTIFY,
    ];
}

impl FromStr for CallFlag {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let flags: Vec<&str> = s.split(',').map(str::trim).collect();
        if flags.is_empty() {
            return Err("empty flags".to_string());
        }
        if flags.len() == 1 {
            for (&f, &str_val) in FLAG_STRING.iter() {
                if s == str_val {
                    return Ok(f);
                }
            }
            return Err("unknown flag".to_string());
        }

        let mut res = CallFlag::NONE;
        for flag in flags {
            let mut known_flag = false;
            for &f in BASIC_FLAGS.iter() {
                if flag == FLAG_STRING[&f] {
                    res = CallFlag(res.0 | f.0);
                    known_flag = true;
                    break;
                }
            }
            if !known_flag {
                return Err("unknown/inappropriate flag".to_string());
            }
        }
        Ok(res)
    }
}

impl fmt::Display for CallFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(&s) = FLAG_STRING.get(self) {
            return write!(f, "{}", s);
        }

        let mut res = String::new();
        let mut remaining = *self;

        for &flag in BASIC_FLAGS.iter() {
            if remaining.has(flag) {
                if !res.is_empty() {
                    res.push_str(", ");
                }
                res.push_str(FLAG_STRING[&flag]);
                remaining = CallFlag(remaining.0 & !flag.0);
            }
        }
        write!(f, "{}", res)
    }
}

impl Serialize for CallFlag {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for CallFlag {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_string() {
        assert_eq!(CallFlag::from_str("ReadStates"), Ok(CallFlag::READ_STATES));
        assert_eq!(CallFlag::from_str("ReadStates, WriteStates"), Ok(CallFlag::STATES));
        assert_eq!(CallFlag::from_str("All"), Ok(CallFlag::ALL));
        assert!(CallFlag::from_str("Unknown").is_err());
    }

    #[test]
    fn test_to_string() {
        assert_eq!(CallFlag::READ_STATES.to_string(), "ReadStates");
        assert_eq!(CallFlag::STATES.to_string(), "States");
        assert_eq!(CallFlag::ALL.to_string(), "All");
    }

    #[test]
    fn test_has() {
        assert!(CallFlag::ALL.has(CallFlag::READ_STATES));
        assert!(CallFlag::ALL.has(CallFlag::WRITE_STATES));
        assert!(!CallFlag::READ_ONLY.has(CallFlag::WRITE_STATES));
    }
}
