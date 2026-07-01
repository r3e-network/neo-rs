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
