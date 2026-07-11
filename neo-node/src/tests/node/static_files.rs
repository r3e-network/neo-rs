//! Static Ledger archive retention-policy tests.

use super::hot_ledger_prune_target;

#[test]
fn prune_target_preserves_exactly_the_traceable_window() {
    assert_eq!(hot_ledger_prune_target(99, 100), None);
    assert_eq!(hot_ledger_prune_target(100, 100), Some(0));
    assert_eq!(hot_ledger_prune_target(101, 100), Some(1));
    assert_eq!(hot_ledger_prune_target(5, 0), Some(5));
}
