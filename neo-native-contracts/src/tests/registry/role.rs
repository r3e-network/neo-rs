use super::Role;

#[test]
fn role_byte_mapping_matches_neo_n3() {
    let cases = [
        (4, Role::StateValidator),
        (8, Role::Oracle),
        (16, Role::NeoFsAlphabetNode),
        (32, Role::P2PNotary),
    ];

    for (value, role) in cases {
        assert_eq!(Role::from_byte(value), Some(role));
        assert_eq!(role.as_byte(), value);
    }

    assert_eq!(Role::from_byte(0), None);
    assert_eq!(Role::from_byte(5), None);
    assert_eq!(Role::from_byte(255), None);
}
