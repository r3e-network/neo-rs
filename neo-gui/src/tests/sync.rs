use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use super::lock;

#[test]
fn lock_recovers_poisoned_mutex() {
    let value = Mutex::new(1_u8);
    let _ = catch_unwind(AssertUnwindSafe(|| {
        let _guard = lock(&value, "test setup");
        panic!("poison mutex");
    }));

    *lock(&value, "test") += 1;

    assert_eq!(*lock(&value, "test"), 2);
}
