use super::ProtocolSettings;
use crate::settings::protocol::overrides::ProtocolSettingsOverrides;

#[test]
fn mainnet_defaults_match() {
    let settings = ProtocolSettings::mainnet();
    assert_eq!(settings.network_magic, 860_833_102);
    assert_eq!(settings.validators_count, 7);
}

#[test]
fn overrides_apply() {
    let settings = ProtocolSettings::mainnet();
    let overrides = ProtocolSettingsOverrides {
        network_magic: Some(1),
        address_version: Some(0x23),
        milliseconds_per_block: Some(10_000),
        ..Default::default()
    };
    let patched = settings.with_overrides(overrides).expect("override");
    assert_eq!(patched.network_magic, 1);
    assert_eq!(patched.address_version, 0x23);
    assert_eq!(patched.milliseconds_per_block, 10_000);
}
