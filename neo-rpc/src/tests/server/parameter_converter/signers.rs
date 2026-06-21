use super::*;

#[test]
fn parse_witness_scope_accepts_names_and_numeric_strings_like_csharp() {
    // Names (existing behavior).
    assert_eq!(
        parse_witness_scope("CalledByEntry").unwrap(),
        WitnessScope::CALLED_BY_ENTRY
    );
    assert_eq!(parse_witness_scope("Global").unwrap(), WitnessScope::GLOBAL);
    assert_eq!(
        parse_witness_scope("CalledByEntry,CustomContracts").unwrap(),
        WitnessScope::from_byte(0x11).unwrap()
    );
    // Numeric strings, as C# Enum.Parse<WitnessScope> accepts.
    assert_eq!(
        parse_witness_scope("1").unwrap(),
        WitnessScope::CALLED_BY_ENTRY
    );
    assert_eq!(parse_witness_scope("128").unwrap(), WitnessScope::GLOBAL);
    assert_eq!(
        parse_witness_scope("17").unwrap(),
        WitnessScope::from_byte(0x11).unwrap()
    );
    // Invalid combinations / tokens still rejected.
    assert!(parse_witness_scope("129").is_err()); // Global cannot combine
    assert!(parse_witness_scope("notascope").is_err());
}
