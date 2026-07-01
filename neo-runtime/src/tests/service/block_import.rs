use super::*;

#[test]
fn imported_tip_uses_block_identity() {
    let mut header = neo_payloads::Header::new();
    header.set_index(42);
    header.set_timestamp(1_700_000);
    let block = Block::from_parts(header, vec![]);

    let tip = ImportedTip::from_block(&block).expect("tip");

    assert_eq!(tip.hash, block.try_hash().expect("hash"));
    assert_eq!(tip.height, 42);
    assert_eq!(tip.timestamp, 1_700_000);
}

#[test]
fn legacy_import_bool_maps_to_typed_outcome() {
    let mut header = neo_payloads::Header::new();
    header.set_index(7);
    let block = Block::from_parts(header, vec![]);
    let hash = block.try_hash().expect("hash");

    assert_eq!(
        BlockImportOutcome::from_legacy_imported(&block, true).expect("imported"),
        BlockImportOutcome::Imported(ImportedTip::from_block(&block).expect("tip"))
    );
    assert_eq!(
        BlockImportOutcome::from_legacy_imported(&block, false).expect("not imported"),
        BlockImportOutcome::NotImported { hash, height: 7 }
    );
}

#[test]
fn batch_outcome_preserves_processed_count() {
    assert_eq!(BlockBatchImportOutcome::new(3).processed, 3);
}
