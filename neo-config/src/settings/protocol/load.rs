//! File and stream loading for `ProtocolSettings`.
//!
//! This module owns C#-compatible path lookup and stream parsing entry points.
//! The JSON section parser and hardfork validation still live in the protocol
//! root until their own focused splits.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{ProtocolConfigError, ProtocolSettings};

impl ProtocolSettings {
    /// Searches for a file in the given path. If not found, checks in the executable directory.
    /// Matches C# FindFile method
    pub fn find_file(file_name: &str, path: &str) -> Option<String> {
        let primary_root = if Path::new(path).is_absolute() {
            PathBuf::from(path)
        } else {
            application_root()
                .map(|root| root.join(path))
                .unwrap_or_else(|| PathBuf::from(path))
        };

        let primary = primary_root.join(file_name);
        if primary.exists() {
            return Some(primary.to_string_lossy().to_string());
        }

        if let Some(exec_root) = application_root() {
            let fallback = exec_root.join(file_name);
            if fallback.exists() {
                return Some(fallback.to_string_lossy().to_string());
            }
        }

        None
    }

    /// Loads the ProtocolSettings from the specified stream.
    /// Matches C# Load(Stream) method
    pub fn load_from_stream(stream: &mut dyn Read) -> Result<Self, ProtocolConfigError> {
        // serde_json::from_reader consumes the stream; seek back to handle reuse over same stream.
        let mut buffered = Vec::new();
        stream.read_to_end(&mut buffered)?;

        if buffered.iter().all(|byte| byte.is_ascii_whitespace()) {
            return Ok(Self::csharp_default());
        }

        let value: Value = serde_json::from_slice(&buffered)?;
        Self::from_value(value)
    }

    /// Loads the ProtocolSettings at the specified path.
    /// Matches C# Load(string) method
    pub fn load(path: &str) -> Result<Self, ProtocolConfigError> {
        let resolved_path = {
            let base_dir = std::env::current_dir()
                .ok()
                .and_then(|dir| dir.to_str().map(|s| s.to_string()));
            match base_dir {
                Some(base) => Self::find_file(path, &base).unwrap_or_else(|| path.to_string()),
                None => path.to_string(),
            }
        };

        if !Path::new(&resolved_path).exists() {
            return Ok(Self::csharp_default());
        }

        let mut file = File::open(&resolved_path)?;
        // Ensure the stream cursor sits at the beginning for delegates expecting fresh readers.
        file.seek(SeekFrom::Start(0))?;
        Self::load_from_stream(&mut file)
    }
}

fn application_root() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}
