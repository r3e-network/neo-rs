use super::*;
use neo_primitives::UInt256;
use neo_storage::persistence::WriteStore;
use neo_storage::persistence::providers::MemoryStore;

fn root(index: u32, byte: u8) -> StateRoot {
    StateRoot::new_current(index, UInt256::from([byte; 32]))
}

#[test]
fn current_index_decoder_requires_exact_width() {
    assert_eq!(
        decode_current_local_root_index(&42u32.to_le_bytes()).expect("decode index"),
        42
    );
    assert!(matches!(
        decode_current_local_root_index(&[0; 3]),
        Err(StateRootRecordError::InvalidCurrentIndexLength { actual: 3 })
    ));
    assert!(matches!(
        decode_current_local_root_index(&[0; 5]),
        Err(StateRootRecordError::InvalidCurrentIndexLength { actual: 5 })
    ));
}

#[test]
fn state_root_decoder_consumes_the_complete_current_record() {
    let expected = root(42, 0x33);
    let decoded = decode_state_root_record(&expected.to_array()).expect("decode root");
    assert_eq!(decoded.version(), CURRENT_VERSION);
    assert_eq!(decoded.index(), 42);
    assert_eq!(decoded.root_hash(), expected.root_hash());

    let mut trailing = expected.to_array();
    trailing.push(0);
    assert!(matches!(
        decode_state_root_record(&trailing),
        Err(StateRootRecordError::TrailingBytes { trailing: 1 })
    ));

    let mut unsupported = expected.to_array();
    unsupported[0] = CURRENT_VERSION.wrapping_add(1);
    assert!(matches!(
        decode_state_root_record(&unsupported),
        Err(StateRootRecordError::UnsupportedVersion { .. })
    ));

    let mut malformed_witness = expected.to_array();
    *malformed_witness.last_mut().expect("witness count") = 2;
    assert!(matches!(
        decode_state_root_record(&malformed_witness),
        Err(StateRootRecordError::Decode { .. })
    ));
}

#[test]
fn local_record_decoder_binds_embedded_index() {
    assert!(matches!(
        decode_local_state_root_record(41, &root(42, 0x44).to_array()),
        Err(StateRootRecordError::IndexMismatch {
            expected: 41,
            actual: 42
        })
    ));
}

#[test]
fn current_root_read_is_strict_about_absence_and_dangling_pointers() {
    let mut store = MemoryStore::new();
    assert!(matches!(
        read_current_local_root(&store),
        Err(StateRootRecordError::MissingCurrentIndex)
    ));

    store
        .put(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            42u32.to_le_bytes().to_vec(),
        )
        .expect("write pointer");
    assert!(matches!(
        read_current_local_root(&store),
        Err(StateRootRecordError::MissingRoot { index: 42 })
    ));

    let expected = root(42, 0x55);
    store
        .put(Keys::state_root(42), expected.to_array())
        .expect("write root");
    let actual = read_current_local_root(&store).expect("read current root");
    assert_eq!(actual.index(), 42);
    assert_eq!(actual.root_hash(), expected.root_hash());
    let historical = read_local_state_root(&store, 42).expect("read indexed root");
    assert_eq!(historical.root_hash(), expected.root_hash());
    assert!(matches!(
        read_local_state_root(&store, 41),
        Err(StateRootRecordError::MissingRoot { index: 41 })
    ));
}

#[test]
fn current_root_read_rejects_malformed_data_instead_of_mapping_it_to_absence() {
    let mut store = MemoryStore::new();
    store
        .put(Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(), vec![0; 3])
        .expect("write malformed pointer");
    assert!(matches!(
        read_current_local_root(&store),
        Err(StateRootRecordError::InvalidCurrentIndexLength { actual: 3 })
    ));

    store
        .put(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            7u32.to_le_bytes().to_vec(),
        )
        .expect("replace pointer");
    store
        .put(Keys::state_root(7), root(8, 0x66).to_array())
        .expect("write mismatched root");
    assert!(matches!(
        read_current_local_root(&store),
        Err(StateRootRecordError::IndexMismatch {
            expected: 7,
            actual: 8
        })
    ));
}
