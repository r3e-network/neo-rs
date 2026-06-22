use super::super::super::NeoFsRange;

#[test]
fn parse_neofs_range_rejects_invalid_format() {
    assert!(NeoFsRange::parse_neofs_range("not-a-range").is_err());
    assert!(NeoFsRange::parse_neofs_range("10|").is_err());
    assert!(NeoFsRange::parse_neofs_range("|10").is_err());
}
