//! Lightweight version representation for wallet subsystems.
//!
//! The C# implementation relies on `System.Version`. We mirror only the
//! functionality that is required by the current Rust port.

use neo_error::{CoreError, CoreResult};
use std::fmt;
use std::str::FromStr;

/// Simple three-component version (major.minor.build).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Version {
    major: u32,
    minor: u32,
    build: u32,
}

impl Version {
    /// Creates a new `Version` instance.
    pub const fn new(major: u32, minor: u32, build: u32) -> Self {
        Self {
            major,
            minor,
            build,
        }
    }

    /// Parses a dotted version string (e.g. "1.0.0").
    pub fn parse(value: &str) -> CoreResult<Self> {
        value.parse()
    }

    /// Returns the major component.
    pub const fn major(&self) -> u32 {
        self.major
    }

    /// Returns the minor component.
    pub const fn minor(&self) -> u32 {
        self.minor
    }

    /// Returns the build component.
    pub const fn build(&self) -> u32 {
        self.build
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.build)
    }
}

impl FromStr for Version {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('.');
        let major = parts
            .next()
            .ok_or_else(|| CoreError::other("missing major component"))?
            .parse()
            .map_err(|e| CoreError::other(format!("invalid major component: {e}")))?;
        let minor = parts
            .next()
            .ok_or_else(|| CoreError::other("missing minor component"))?
            .parse()
            .map_err(|e| CoreError::other(format!("invalid minor component: {e}")))?;
        let build = parts
            .next()
            .unwrap_or("0")
            .parse()
            .map_err(|e| CoreError::other(format!("invalid build component: {e}")))?;

        if parts.next().is_some() {
            return Err(CoreError::other("too many components"));
        }

        Ok(Self::new(major, minor, build))
    }
}
