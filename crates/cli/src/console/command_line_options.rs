use neo_extensions::LogLevel;

/// Mirrors `Neo.CLI/CLI/CommandLineOption.cs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLineOptions {
    pub config: Option<String>,
    pub wallet: Option<String>,
    pub password: Option<String>,
    pub plugins: Vec<String>,
    pub db_engine: Option<String>,
    pub db_path: Option<String>,
    pub verbose: LogLevel,
    pub no_verify: Option<bool>,
    pub background: bool,
}

impl CommandLineOptions {
    /// Returns `true` when at least one option was specified.
    pub fn is_valid(&self) -> bool {
        self.config.is_some()
            || self.wallet.is_some()
            || self.password.is_some()
            || self.db_engine.is_some()
            || self.db_path.is_some()
            || !self.plugins.is_empty()
            || self.no_verify.is_some()
            || self.background
            || self.verbose != LogLevel::Info
    }
}

impl Default for CommandLineOptions {
    fn default() -> Self {
        Self {
            config: None,
            wallet: None,
            password: None,
            plugins: Vec::new(),
            db_engine: None,
            db_path: None,
            verbose: LogLevel::Info,
            no_verify: None,
            background: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_matches_csharp_semantics() {
        let mut options = CommandLineOptions::default();
        assert!(!options.is_valid());
        options.config = Some("neo.toml".to_string());
        assert!(options.is_valid());
    }
}
