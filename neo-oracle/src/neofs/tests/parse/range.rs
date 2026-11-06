use super::super::super::parse::parse_neofs_range;

#[test]
fn parse_neofs_range_rejects_invalid_format() {
    assert!(parse_neofs_range("not-a-range").is_err());
    assert!(parse_neofs_range("10|").is_err());
    assert!(parse_neofs_range("|10").is_err());
}
