use super::*;
// The `invocation_script`/`verification_script` accessors on the built
// `Witness` come from the `neo_primitives::Witness` trait.
use neo_primitives::Witness as _;

#[test]
fn empty_builder_produces_empty_witness() {
    let w = WitnessBuilder::new().build();
    assert!(w.invocation_script().is_empty());
    assert!(w.verification_script().is_empty());
}

#[test]
fn consuming_setters_populate_scripts() {
    let w = WitnessBuilder::new()
        .invocation_script(vec![1, 2, 3])
        .verification_script(vec![4, 5])
        .build();
    assert_eq!(w.invocation_script(), &[1, 2, 3]);
    assert_eq!(w.verification_script(), &[4, 5]);
}

#[test]
fn add_is_set_once_per_slot() {
    let mut b = WitnessBuilder::new();
    b.add_invocation(vec![1]).unwrap();
    // A second add for an already-populated slot is rejected.
    assert!(b.add_invocation(vec![2]).is_err());
    // The verification slot is independent and still settable.
    b.add_verification(vec![9]).unwrap();
    let w = b.build();
    assert_eq!(w.invocation_script(), &[1]);
    assert_eq!(w.verification_script(), &[9]);
}

#[test]
fn add_with_builder_emits_script_bytes() {
    let mut b = WitnessBuilder::new();
    b.add_verification_with_builder(|sb| {
        sb.emit_push_int(1);
    })
    .unwrap();
    // The closure-built verification script is non-empty.
    assert!(!b.build().verification_script().is_empty());
}
