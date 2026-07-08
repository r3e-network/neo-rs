use super::*;

#[test]
fn derive_matches_create_with_path_for_same_child_index() {
    let seed = [0x42u8; 64];

    let master = ExtendedKey::create(&seed, None).expect("master key");
    let direct_child = master.derive(1).expect("direct child");
    let path_child = ExtendedKey::create_with_path(&seed, "m/1", None).expect("path child");

    assert_eq!(direct_child.private_key(), path_child.private_key());
    assert_eq!(direct_child.chain_code(), path_child.chain_code());
    assert_eq!(
        direct_child.public_key().as_bytes(),
        path_child.public_key().as_bytes()
    );
    assert_eq!(direct_child.private_key().len(), 32);
    assert_eq!(direct_child.chain_code().len(), 32);
    assert_ne!(direct_child.private_key(), master.private_key());
    assert_ne!(direct_child.chain_code(), master.chain_code());
}
