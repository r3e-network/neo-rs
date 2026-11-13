//! Owner-only filesystem permissions for TEE host artifacts.
//!
//! See `docs/tee-fs-acl.md`.

use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

/// Restrict `path` so only the current owner/user can access it.
pub fn restrict_owner_only(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if path.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }

    #[cfg(windows)]
    {
        restrict_windows_acl(path)
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Ok(())
    }
}

/// Write `data` to `path` (create/truncate), then restrict to owner-only access.
pub fn write_owner_only(path: &Path, data: &[u8]) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.write_all(data)?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)?;
            file.write_all(data)?;
        }
        restrict_owner_only(path)
    }
}

#[cfg(windows)]
fn current_windows_user() -> io::Result<String> {
    let user = std::env::var("USERNAME").map_err(|_| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "USERNAME environment variable is required to set Windows ACLs",
        )
    })?;
    Ok(match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
        _ => user,
    })
}

#[cfg(windows)]
fn run_icacls(args: &[&str]) -> io::Result<()> {
    let output = Command::new("icacls").args(args).output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(io::Error::other(format!(
            "icacls {} failed: {}",
            args.join(" "),
            stderr.trim()
        )))
    }
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path) -> io::Result<()> {
    let path_str = path.to_string_lossy();
    let user = current_windows_user()?;
    let grant = if path.is_dir() {
        format!("{user}:(OI)(CI)F")
    } else {
        format!("{user}:F")
    };

    // Ensure the current user has an explicit ACE before stripping inheritance.
    // If inheritance is removed first and grant fails, the file can become
    // permanently inaccessible to this process.
    run_icacls(&[path_str.as_ref(), "/grant:r", &grant])?;
    run_icacls(&[path_str.as_ref(), "/inheritance:r"])?;
    // Re-assert the owner grant after inheritance conversion (explicit ACE set).
    run_icacls(&[path_str.as_ref(), "/grant:r", &grant])?;

    // Best-effort removal of broad principals that may remain as explicit ACEs.
    for principal in ["Everyone", "BUILTIN\\Users", "Authenticated Users"] {
        let _ = run_icacls(&[path_str.as_ref(), "/remove:g", principal]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn restrict_owner_only_roundtrip() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("secret.bin");
        write_owner_only(&file, b"tee-secret").expect("write_owner_only");
        assert_eq!(fs::read(&file).unwrap(), b"tee-secret");
        restrict_owner_only(dir.path()).expect("restrict dir");
    }
}
