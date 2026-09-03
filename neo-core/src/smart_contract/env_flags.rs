pub(crate) fn env_flag_enabled(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|raw| parse_env_flag(&raw))
        .unwrap_or(default)
}

fn parse_env_flag(value: &str) -> bool {
    let trimmed = value.trim();
    ["1", "true", "yes", "on"]
        .iter()
        .any(|truthy| trimmed.eq_ignore_ascii_case(truthy))
}

#[cfg(test)]
mod tests {
    use super::parse_env_flag;

    #[test]
    fn parse_env_flag_accepts_existing_truthy_values() {
        for value in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(parse_env_flag(value));
        }
    }

    #[test]
    fn parse_env_flag_rejects_other_values() {
        for value in ["", "0", "false", "off", "enabled", " true-ish "] {
            assert!(!parse_env_flag(value));
        }
    }
}
