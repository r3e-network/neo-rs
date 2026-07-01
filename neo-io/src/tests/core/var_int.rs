use super::VarInt;

#[test]
fn reads_var_int_prefix_without_consuming() {
    let cases: &[(&[u8], u64, usize)] = &[
        (&[0xFC], 0xFC, 1),
        (&[0xFD, 0xFD, 0x00], 0xFD, 3),
        (&[0xFD, 0xFF, 0xFF], 0xFFFF, 3),
        (&[0xFE, 0x00, 0x00, 0x01, 0x00], 0x1_0000, 5),
        (&[0xFE, 0xFF, 0xFF, 0xFF, 0xFF], 0xFFFF_FFFF, 5),
        (&[0xFF, 0, 0, 0, 0, 1, 0, 0, 0], 0x1_0000_0000, 9),
    ];

    for (encoded, value, width) in cases {
        assert_eq!(VarInt::read_var_int_prefix(encoded), Some((*value, *width)));
    }
}

#[test]
fn waits_for_incomplete_var_int_prefix() {
    for encoded in [
        &[][..],
        &[0xFD][..],
        &[0xFD, 0x01][..],
        &[0xFE, 0, 0, 0][..],
        &[0xFF, 0, 0, 0, 0, 0, 0, 0][..],
    ] {
        assert_eq!(VarInt::read_var_int_prefix(encoded), None);
    }
}

#[test]
fn reads_non_canonical_prefix_for_legacy_compatibility() {
    let cases: &[(&[u8], u64, usize)] = &[
        (&[0xFD, 0x01, 0x00], 1, 3),
        (&[0xFE, 0x01, 0x00, 0x00, 0x00], 1, 5),
        (
            &[0xFF, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00],
            1,
            9,
        ),
    ];

    for (encoded, value, width) in cases {
        assert_eq!(VarInt::read_var_int_prefix(encoded), Some((*value, *width)));
    }
}

#[test]
fn writes_canonical_var_int_encoding() {
    let cases: &[(u64, &[u8])] = &[
        (0xFC, &[0xFC]),
        (0xFD, &[0xFD, 0xFD, 0x00]),
        (0xFFFF, &[0xFD, 0xFF, 0xFF]),
        (0x1_0000, &[0xFE, 0x00, 0x00, 0x01, 0x00]),
        (0xFFFF_FFFF, &[0xFE, 0xFF, 0xFF, 0xFF, 0xFF]),
        (0x1_0000_0000, &[0xFF, 0, 0, 0, 0, 1, 0, 0, 0]),
    ];

    for (value, expected) in cases {
        let mut encoded = Vec::new();
        VarInt::write_var_int(*value, &mut encoded);
        assert_eq!(&encoded, expected);
    }
}

#[test]
fn calculates_encoded_var_int_length() {
    let cases = [
        (0, 1),
        (0xFC, 1),
        (0xFD, 3),
        (0xFFFF, 3),
        (0x1_0000, 5),
        (0xFFFF_FFFF, 5),
        (0x1_0000_0000, 9),
    ];

    for (value, expected) in cases {
        assert_eq!(VarInt::encoded_len(value), expected);
    }
}

#[test]
fn writes_var_bytes_without_intermediate_writer() {
    let mut encoded = Vec::new();

    VarInt::write_var_bytes(&[0xAA, 0xBB], &mut encoded);

    assert_eq!(encoded, vec![0x02, 0xAA, 0xBB]);
}
