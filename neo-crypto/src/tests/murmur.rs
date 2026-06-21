use super::Murmur3;

#[test]
fn test_murmur128_vectors() {
    let hex_input = hex::decode("718f952132679baa9c5c2aa0d329fd2a").unwrap();
    let cases: Vec<(&[u8], &str)> = vec![
        (b"hello", "0bc59d0ad25fde2982ed65af61227a0e"),
        (b"world", "3d3810fed480472bd214a14023bb407f"),
        (b"hello world", "e0a0632d4f51302c55e3b3e48d28795d"),
        (&hex_input, "9b4aa747ff0cf4e41b3d96251551c8ae"),
    ];

    for (input, expected) in cases {
        let hash = Murmur3::murmur128(input, 123u32);
        assert_eq!(hex::encode(hash), expected);
    }
}
