//! Hierarchical actor path: an actor system name plus its child segments.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActorPath {
    system: String,
    segments: Vec<String>,
}

impl ActorPath {
    pub fn new(system: impl Into<String>, segments: Vec<String>) -> Self {
        Self {
            system: system.into(),
            segments,
        }
    }

    pub fn root(system: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            segments: vec![name.into()],
        }
    }

    pub fn child(&self, name: impl Into<String>) -> Self {
        let mut segments = self.segments.clone();
        segments.push(name.into());
        Self {
            system: self.system.clone(),
            segments,
        }
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    pub fn system(&self) -> &str {
        &self.system
    }

    pub fn parse(path: &str) -> Option<Self> {
        let mut parts = path.split('/').filter(|p| !p.is_empty());
        let system = parts.next()?.to_string();
        let segments: Vec<String> = parts.map(|p| p.to_string()).collect();
        if segments.is_empty() {
            return None;
        }
        Some(Self { system, segments })
    }
}

impl fmt::Display for ActorPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}/{}", self.system, self.segments.join("/"))
    }
}
