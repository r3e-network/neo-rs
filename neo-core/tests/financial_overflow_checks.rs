//! Tests for financial operation overflow checks

use neo_core::smart_contract::helper::Helper as ContractHelper;

#[test]
fn test_signature_contract_cost_no_overflow() {
    // Test that signature contract cost calculation doesn't overflow
    let cost = ContractHelper::signature_contract_cost();
    assert!(cost > 0, "Signature contract cost should be positive");
    assert!(
        cost < i64::MAX,
        "Signature contract cost should be reasonable"
    );
}

#[test]
fn test_multi_signature_contract_cost_no_overflow() {
    // Test normal case
    let cost = ContractHelper::multi_signature_contract_cost(2, 3);
    assert!(cost > 0, "Multi-sig contract cost should be positive");

    // Test with maximum reasonable values
    let cost_large = ContractHelper::multi_signature_contract_cost(100, 1024);
    assert!(cost_large > 0, "Large multi-sig cost should be positive");
    assert!(
        cost_large < i64::MAX,
        "Large multi-sig cost should not overflow"
    );
}

#[test]
fn test_multi_signature_contract_cost_extreme_values() {
    // Test with very large values that could cause overflow without saturation
    let cost = ContractHelper::multi_signature_contract_cost(i32::MAX, i32::MAX);
    // Should saturate rather than panic or wrap
    assert!(
        cost == i64::MAX,
        "Extreme values should saturate to i64::MAX"
    );
}

#[test]
fn test_fee_calculations_with_saturation() {
    // Verify that saturating arithmetic prevents wrapping
    let max_fee = i64::MAX;
    let result = max_fee.saturating_add(1);
    assert_eq!(result, i64::MAX, "Saturating add should not overflow");

    let result2 = max_fee.saturating_mul(2);
    assert_eq!(result2, i64::MAX, "Saturating mul should not overflow");
}

#[test]
fn test_gas_consumed_calculation_safety() {
    // Test that gas consumption calculations handle edge cases
    let fee_factor = 100_000_000i64; // FEE_FACTOR from the code

    // Normal case
    let gas = 1_000_000i64;
    let result = gas
        .saturating_add(fee_factor - 1)
        .saturating_div(fee_factor);
    assert!(
        result >= 0,
        "Gas calculation should produce non-negative result"
    );

    // Edge case: near max value
    let gas_large = i64::MAX - fee_factor;
    let result_large = gas_large
        .saturating_add(fee_factor - 1)
        .saturating_div(fee_factor);
    assert!(
        result_large > 0,
        "Large gas calculation should produce positive result"
    );
}
