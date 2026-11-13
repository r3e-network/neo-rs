//! Append-only security and validation audit records. Secrets and RPC bodies are excluded.

use anyhow::{Context, Result};
use parking_lot::Mutex;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone)]
pub struct AuditLog {
    file: Arc<Mutex<File>>,
}

impl AuditLog {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create audit directory {}", parent.display())
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open audit log {}", path.display()))?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn append<T: Serialize>(&self, action: &str, record: &T) -> Result<()> {
        #[derive(Serialize)]
        struct Envelope<'a, T> {
            observed_at_unix_ms: u64,
            action: &'a str,
            record: &'a T,
        }
        let envelope = Envelope {
            observed_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
            action,
            record,
        };
        let mut bytes = serde_json::to_vec(&envelope)?;
        bytes.push(b'\n');
        let mut file = self.file.lock();
        file.write_all(&bytes)?;
        file.flush()?;
        Ok(())
    }
}
