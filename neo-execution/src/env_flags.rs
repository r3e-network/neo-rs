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
#[path = "tests/env_flags.rs"]
mod tests;
