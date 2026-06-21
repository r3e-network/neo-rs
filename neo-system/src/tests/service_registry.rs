use super::*;

#[derive(Debug, PartialEq)]
struct Alpha(u32);

#[derive(Debug, PartialEq)]
struct Beta(&'static str);

#[test]
fn empty_registry_returns_none() {
    let registry = ServiceRegistry::new();
    assert!(registry.get::<Alpha>().is_none());
    assert!(!registry.contains::<Alpha>());
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
}

#[test]
fn register_then_get_round_trips_by_type() {
    let registry = ServiceRegistry::new();
    registry.register(Arc::new(Alpha(7)));
    registry.register(Arc::new(Beta("b")));

    assert_eq!(registry.get::<Alpha>().as_deref(), Some(&Alpha(7)));
    assert_eq!(registry.get::<Beta>().as_deref(), Some(&Beta("b")));
    assert_eq!(registry.len(), 2);
    assert!(registry.contains::<Alpha>());
}

#[test]
fn register_replaces_and_returns_previous_instance() {
    let registry = ServiceRegistry::new();
    assert!(registry.register(Arc::new(Alpha(1))).is_none());
    let previous = registry.register(Arc::new(Alpha(2)));
    assert_eq!(previous.as_deref(), Some(&Alpha(1)));
    assert_eq!(registry.get::<Alpha>().as_deref(), Some(&Alpha(2)));
    assert_eq!(registry.len(), 1);
}

#[test]
fn clones_share_the_same_map() {
    let registry = ServiceRegistry::new();
    let clone = registry.clone();
    registry.register(Arc::new(Alpha(42)));
    assert_eq!(clone.get::<Alpha>().as_deref(), Some(&Alpha(42)));
}

#[test]
fn registered_arc_is_shared_not_copied() {
    let registry = ServiceRegistry::new();
    let service = Arc::new(Alpha(9));
    registry.register(Arc::clone(&service));
    let fetched = registry.get::<Alpha>().expect("registered");
    assert!(Arc::ptr_eq(&service, &fetched));
}
