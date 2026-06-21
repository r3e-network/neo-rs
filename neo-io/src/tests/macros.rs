#[test]
fn test_impl_ord_by_fields() {
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Point {
        x: i32,
        y: i32,
    }

    impl_ord_by_fields!(Point, x, y);

    let p1 = Point { x: 1, y: 2 };
    let p2 = Point { x: 1, y: 3 };
    let p3 = Point { x: 2, y: 1 };

    assert!(p1 < p2);
    assert!(p2 < p3);
    assert!(p1 < p3);
}
