use super::*;
use neo_vm_rs::StackValue;

#[test]
fn integer_length_uses_neo_vm_rs_byte_string_rules() {
    let validator = MaxLengthAttribute::new(1);

    assert!(validator.validate(&StackValue::Integer(127)).is_ok());
    assert!(validator.validate(&StackValue::Integer(128)).is_err());
}

#[test]
fn compound_lengths_use_stack_value_shapes() {
    let validator = MaxLengthAttribute::new(1);

    assert!(
        validator
            .validate(&StackValue::Array(vec![StackValue::Null]))
            .is_ok()
    );
    assert!(
        validator
            .validate(&StackValue::Struct(vec![
                StackValue::Null,
                StackValue::Null
            ]))
            .is_err()
    );
    assert!(
        validator
            .validate(&StackValue::Map(vec![
                (StackValue::Integer(1), StackValue::Boolean(true)),
                (StackValue::Integer(2), StackValue::Boolean(false)),
            ]))
            .is_err()
    );
}
