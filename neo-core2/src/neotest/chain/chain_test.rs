use std::sync::Arc;
use neo_test::{NewMulti, NewExecutor};
use neo_test::require;
use neo_test::testing::Test;

#[test]
fn test_new_multi() {
    let (bc, v_acc, c_acc) = NewMulti::new();
    let e = NewExecutor::new(bc.clone(), v_acc.clone(), c_acc.clone());

    require::not_equal(v_acc.script_hash(), c_acc.script_hash());

    const AMOUNT: i64 = 10_0000_0000;

    let c = e.committee_invoker(bc.utility_token_hash()).with_signers(v_acc.clone());
    c.invoke(true, "transfer", e.validator().script_hash(), e.committee().script_hash(), AMOUNT, None);
}
