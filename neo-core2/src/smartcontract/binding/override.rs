use std::str::FromStr;
use serde::{Deserialize, Serialize};

/// Override contains a package and a type to replace manifest method parameter type with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    /// Package contains a fully-qualified package name.
    pub package: String,
    /// TypeName contains type name together with a package alias.
    pub type_name: String,
}

impl Override {
    /// Creates a new Override from a string.
    pub fn new_from_string(s: &str) -> Self {
        let mut over = Override {
            package: String::new(),
            type_name: String::new(),
        };

        if let Some(index) = s.rfind('.') {
            // Arrays and maps can have fully-qualified types as elements.
            let last = s.rfind(|c| c == ']' || c == '*');
            let is_compound = last.map_or(false, |last| last < index);

            if is_compound {
                over.package = s[last.unwrap() + 1..index].to_string();
            } else {
                over.package = s[..index].to_string();
            }

            match over.package.as_str() {
                "iterator" | "storage" => {
                    over.package = format!("github.com/nspcc-dev/neo-go/pkg/interop/{}", over.package);
                }
                "ledger" | "management" => {
                    over.package = format!("github.com/nspcc-dev/neo-go/pkg/interop/native/{}", over.package);
                }
                _ => {}
            }

            let slash_index = s.rfind('/').unwrap_or(0);
            if is_compound {
                over.type_name = format!("{}{}", &s[..last.unwrap() + 1], &s[slash_index + 1..]);
            } else {
                over.type_name = s[slash_index + 1..].to_string();
            }
        } else {
            over.type_name = s.to_string();
        }

        over
    }
}

impl FromStr for Override {
    type Err = serde_yaml::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Override::new_from_string(s))
    }
}

impl Serialize for Override {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.package.is_empty() {
            serializer.serialize_str(&self.type_name)
        } else {
            let index = self.type_name.rfind('.').unwrap_or(0);
            let last = self.type_name.rfind(|c| c == ']' || c == '*');
            
            if let Some(last) = last {
                let result = format!("{}{}{}", &self.type_name[..last + 1], self.package, &self.type_name[index..]);
                serializer.serialize_str(&result)
            } else {
                let result = format!("{}{}", self.package, &self.type_name[index..]);
                serializer.serialize_str(&result)
            }
        }
    }
}
