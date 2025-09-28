// Copyright (C) 2015-2025 The Neo Project.
//
// assembly_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Assembly extensions matching C# AssemblyExtensions exactly
pub trait AssemblyExtensions {
    /// Gets the version of the assembly.
    /// Matches C# GetVersion method
    fn get_version(&self) -> String;
}

impl AssemblyExtensions for str {
    fn get_version(&self) -> String {
        resolve_version(self).unwrap_or_else(default_package_version)
    }
}

impl AssemblyExtensions for String {
    fn get_version(&self) -> String {
        self.as_str().get_version()
    }
}

fn resolve_version(input: &str) -> Option<String> {
    if input.trim().is_empty() {
        return None;
    }

    let path = std::path::Path::new(input);
    let search_paths = if path.exists() {
        vec![path.to_path_buf()]
    } else {
        executable_directory()
            .map(|exe_dir| vec![exe_dir.join(input)])
            .unwrap_or_default()
    };

    for candidate in search_paths {
        if let Some(version) = version_from_path(&candidate) {
            return Some(version);
        }
    }

    None
}

fn version_from_path(path: &std::path::Path) -> Option<String> {
    if path.is_dir() {
        let manifest = path.join("Cargo.toml");
        return version_from_manifest(&manifest);
    }

    if path.file_name()? == "Cargo.toml" {
        return version_from_manifest(path);
    }

    // If this is a binary, try searching for Cargo.toml next to it.
    if let Some(parent) = path.parent() {
        let manifest = parent.join("Cargo.toml");
        if let Some(version) = version_from_manifest(&manifest) {
            return Some(version);
        }
    }

    None
}

fn version_from_manifest(manifest: &std::path::Path) -> Option<String> {
    if !manifest.is_file() {
        return None;
    }

    let manifest_contents = std::fs::read_to_string(manifest).ok()?;
    let value: toml::Value = toml::from_str(&manifest_contents).ok()?;
    value
        .get("package")?
        .get("version")?
        .as_str()
        .map(|s| s.to_string())
}

fn executable_directory() -> Option<std::path::PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
}

fn default_package_version() -> String {
    option_env!("CARGO_PKG_VERSION")
        .unwrap_or("0.0.0")
        .to_string()
}
