use neo_extensions::plugin::plugins_directory;
use std::env;
use tempfile::TempDir;

#[test]
fn plugins_directory_can_be_overridden_with_env() {
    let override_dir = TempDir::new().expect("temp dir");
    env::set_var("NEO_PLUGINS_DIR", override_dir.path());

    let resolved = plugins_directory();

    assert_eq!(resolved, override_dir.path());
    env::remove_var("NEO_PLUGINS_DIR");
}

#[test]
fn plugins_directory_defaults_to_application_root() {
    env::remove_var("NEO_PLUGINS_DIR");
    let resolved = plugins_directory();
    assert!(resolved.ends_with("Plugins"));
}
