use super::CommandResult;
use crate::{config::PluginsSection, console_service::ConsoleHelper};
use anyhow::{anyhow, Context};
use neo_extensions::plugin::{plugins_directory, PluginInfo};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeSet, HashSet},
    env, fs,
    future::Future,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    thread,
};
use zip::ZipArchive;

/// Plugin commands (`MainService.Plugins`).
pub struct PluginCommands {
    http: Client,
    download_url: String,
    version_tag: String,
    prerelease: bool,
}

impl PluginCommands {
    pub fn new(config: &PluginsSection) -> Self {
        let version = config
            .version
            .as_deref()
            .unwrap_or(env!("CARGO_PKG_VERSION"));
        let version_tag = format!("v{}", normalize_version(version));
        let user_agent = format!("neo-cli/{}", env!("CARGO_PKG_VERSION"));
        let http = thread::spawn(move || Client::builder().user_agent(user_agent).build())
            .join()
            .expect("HTTP client builder thread panicked")
            .expect("failed to build HTTP client for plugin commands");
        Self {
            http,
            download_url: config.download_url.clone(),
            version_tag,
            prerelease: config.prerelease,
        }
    }

    pub fn list_plugins(&self) -> CommandResult {
        let available = self.fetch_available_plugins().map_err(|err| {
            anyhow!(
                "failed to query plugin catalog at {}: {err}",
                self.download_url
            )
        })?;
        let installed = installed_plugins();
        let mut active = HashSet::new();
        match block_on(neo_extensions::plugin::global_plugin_infos()) {
            Ok(infos) => {
                for info in infos {
                    active.insert(info.name.to_ascii_lowercase());
                }
            }
            Err(err) => ConsoleHelper::warning(format!("failed to query active plugins: {err}")),
        }

        let mut installed_set = HashSet::new();
        for name in &installed {
            installed_set.insert(name.to_ascii_lowercase());
        }

        let mut names = BTreeSet::new();
        for name in &available {
            names.insert(name.clone());
        }
        for name in &installed {
            names.insert(name.clone());
        }

        let max_len = names.iter().map(|name| name.len()).max().unwrap_or(0);
        for name in names {
            let lower = name.to_ascii_lowercase();
            let padded = format!("{name:<width$}", width = max_len);
            let status = if active.contains(&lower) {
                "[Active]"
            } else if installed_set.contains(&lower) {
                "[Installed]"
            } else {
                "[Not Installed]"
            };
            print_line(&format!("{status}\t {padded}"));
        }
        Ok(())
    }

    pub fn list_loaded_plugins(&self) -> CommandResult {
        let plugins: Vec<PluginInfo> = block_on(neo_extensions::plugin::global_plugin_infos())
            .map_err(|err| anyhow!("failed to query loaded plugins: {}", err))?;

        if plugins.is_empty() {
            print_line("No plugins currently loaded");
            return Ok(());
        }

        let mut lines: Vec<String> = plugins
            .into_iter()
            .map(|info| {
                format!(
                    "{} v{} ({})",
                    info.name,
                    info.version,
                    category_label(&info.category)
                )
            })
            .collect();
        lines.sort();
        for line in lines {
            print_line(&line);
        }
        Ok(())
    }

    pub fn install_plugin(&self, name: &str, download_url: Option<&str>) -> CommandResult {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("plugin name cannot be empty"));
        }
        let mut visited = HashSet::new();
        if plugin_directory(trimmed).exists() {
            ConsoleHelper::warning("Plugin already exist.");
            return Ok(());
        }
        self.install_recursive(trimmed, download_url, &mut visited, false)
    }

    pub fn reinstall_plugin(&self, name: &str, download_url: Option<&str>) -> CommandResult {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("plugin name cannot be empty"));
        }
        let mut visited = HashSet::new();
        self.install_recursive(trimmed, download_url, &mut visited, true)
    }

    pub fn uninstall_plugin(&self, name: &str) -> CommandResult {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("plugin name cannot be empty"));
        }
        let plugin_dir = plugin_directory(trimmed);
        if !plugin_dir.exists() {
            ConsoleHelper::warning(format!("plugin '{trimmed}' is not installed"));
            return Ok(());
        }

        let dependents = dependent_plugins(trimmed)?;
        if !dependents.is_empty() {
            ConsoleHelper::error(format!(
                "{trimmed} is required by other plugins: {}",
                dependents.join(", ")
            ));
            ConsoleHelper::info(["Info: ", "If plugin is damaged try reinstalling it."]);
            return Err(anyhow!("plugin has dependencies"));
        }

        fs::remove_dir_all(&plugin_dir).with_context(|| {
            format!("failed to remove plugin directory {}", plugin_dir.display())
        })?;
        ConsoleHelper::info([
            "",
            &format!("Uninstall successful, please restart \"{}\".", cli_name()),
        ]);
        Ok(())
    }

    fn fetch_available_plugins(&self) -> Result<Vec<String>, reqwest::Error> {
        let Some(release) = self.fetch_target_release(None)? else {
            return Ok(Vec::new());
        };
        Ok(asset_names(&release))
    }

    fn install_recursive(
        &self,
        name: &str,
        download_url: Option<&str>,
        visited: &mut HashSet<String>,
        overwrite: bool,
    ) -> CommandResult {
        let normalized = name.to_ascii_lowercase();
        if !visited.insert(normalized.clone()) {
            return Ok(());
        }

        let plugin_dir = plugin_directory(name);
        if plugin_dir.exists() {
            if overwrite {
                fs::remove_dir_all(&plugin_dir).with_context(|| {
                    format!(
                        "failed to remove existing plugin directory {}",
                        plugin_dir.display()
                    )
                })?;
            } else {
                ConsoleHelper::warning(format!("plugin '{name}' already installed"));
                return Ok(());
            }
        }

        let archive = self.download_plugin_archive(name, download_url)?;
        let dependencies = dependencies_from_archive(&archive)?;
        for dependency in dependencies {
            self.install_recursive(&dependency, download_url, visited, false)?;
        }

        extract_archive(&archive, &installation_root())
            .with_context(|| format!("failed to extract archive for plugin '{}'", name))?;
        let action = if overwrite { "Reinstall" } else { "Install" };
        ConsoleHelper::info([
            "",
            &format!("{action} successful, please restart \"{}\".", cli_name()),
        ]);
        Ok(())
    }

    fn download_plugin_archive(
        &self,
        name: &str,
        download_url: Option<&str>,
    ) -> anyhow::Result<Vec<u8>> {
        let release = self
            .fetch_target_release(download_url)
            .map_err(|err| anyhow!(err))?
            .ok_or_else(|| anyhow!("no release found for {}", self.version_tag))?;

        let Some(asset) = release
            .assets
            .into_iter()
            .find(|asset| asset_matches(&asset.name, name))
        else {
            return Err(anyhow!(
                "plugin '{}' not found in release {}; check name and version",
                name,
                self.version_tag
            ));
        };

        let response = self
            .http
            .get(&asset.browser_download_url)
            .send()
            .map_err(|err| anyhow!("failed to download {}: {}", asset.name, err))?
            .error_for_status()
            .map_err(|err| anyhow!("failed to download {}: {}", asset.name, err))?;
        let bytes = response
            .bytes()
            .map_err(|err| anyhow!("failed to read plugin archive: {}", err))?;
        Ok(bytes.to_vec())
    }

    fn fetch_target_release(
        &self,
        download_url: Option<&str>,
    ) -> Result<Option<Release>, reqwest::Error> {
        let url = download_url.unwrap_or(&self.download_url);
        let response = self.http.get(url).send()?.error_for_status()?;
        let releases: Vec<Release> = response.json()?;
        Ok(releases.into_iter().find(|release| {
            release.tag_name == self.version_tag && release.prerelease == self.prerelease
        }))
    }
}

fn installed_plugins() -> Vec<String> {
    let mut list = Vec::new();
    let directory = plugins_directory();
    if let Ok(entries) = fs::read_dir(&directory) {
        for entry in entries.flatten() {
            if entry.file_type().map(|ty| ty.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    list.push(name.to_string());
                }
            }
        }
    }
    list
}

fn plugin_directory(name: &str) -> PathBuf {
    plugins_directory().join(name)
}

fn normalize_version(version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or("0");
    let minor = parts.next().unwrap_or("0");
    let patch = parts.next().unwrap_or("0");
    format!("{major}.{minor}.{patch}")
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    #[allow(dead_code)]
    browser_download_url: String,
}

fn print_line(message: &str) {
    ConsoleHelper::info(["", message]);
}

fn dependent_plugins(name: &str) -> anyhow::Result<Vec<String>> {
    let mut dependents = Vec::new();
    let target = name.to_ascii_lowercase();
    for entry in installed_plugins() {
        let root = plugin_directory(&entry);
        let config_file = find_config_json(&root);
        if config_file.is_none() {
            continue;
        }
        let config_path = root.join(config_file.unwrap());
        let contents = fs::read_to_string(&config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;
        if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&contents) {
            if let Some(Value::Array(deps)) = map.get("Dependency") {
                for dep in deps.iter().filter_map(|value| value.as_str()) {
                    if dep.to_ascii_lowercase() == target {
                        dependents.push(entry.clone());
                        break;
                    }
                }
            }
        }
    }
    Ok(dependents)
}

fn asset_matches(asset_name: &str, plugin_name: &str) -> bool {
    Path::new(asset_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.eq_ignore_ascii_case(plugin_name))
        .unwrap_or(false)
}

fn asset_names(release: &Release) -> Vec<String> {
    release
        .assets
        .iter()
        .filter_map(|asset| {
            Path::new(&asset.name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_string())
        })
        .filter(|name| !name.to_ascii_lowercase().starts_with("neo-cli"))
        .collect()
}

fn dependencies_from_archive(bytes: &[u8]) -> anyhow::Result<Vec<String>> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().rsplit('/').next().unwrap_or_default();
        if !name.eq_ignore_ascii_case("config.json") {
            continue;
        }
        let mut contents = String::new();
        entry.read_to_string(&mut contents)?;
        let value: Value = serde_json::from_str(&contents)?;
        if let Some(Value::Array(deps)) = value.get("Dependency") {
            let mut result = Vec::new();
            for dep in deps {
                if let Some(name) = dep.as_str() {
                    result.push(name.to_string());
                }
            }
            return Ok(result);
        }
    }
    Ok(Vec::new())
}

fn extract_archive(bytes: &[u8], destination: &Path) -> anyhow::Result<()> {
    let cursor = Cursor::new(bytes);
    let mut archive = ZipArchive::new(cursor)?;
    fs::create_dir_all(destination).ok();
    archive
        .extract(destination)
        .map_err(|err| anyhow!("failed to extract archive: {}", err))
}

fn installation_root() -> PathBuf {
    env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn cli_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

fn block_on<T>(future: impl Future<Output = T>) -> Result<T, anyhow::Error> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        Ok(handle.block_on(future))
    } else {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .context("failed to build tokio runtime")?;
        Ok(rt.block_on(future))
    }
}

fn category_label(category: &neo_extensions::plugin::PluginCategory) -> &'static str {
    use neo_extensions::plugin::PluginCategory::*;
    match category {
        Core => "core",
        Network => "network",
        Consensus => "consensus",
        Rpc => "rpc",
        Wallet => "wallet",
        Storage => "storage",
        Utility => "utility",
        Custom(_) => "custom",
    }
}

fn find_config_json(root: &Path) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.eq_ignore_ascii_case("config.json"))
                .unwrap_or(false)
            {
                return path.strip_prefix(root).map(Path::to_path_buf).ok();
            }
        }
    }
    None
}
