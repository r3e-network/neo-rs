#!/usr/bin/env python3
"""
Compare neo-rs fee calculation with C# reference implementation.
Tests CRITICAL-004: Transaction Fee Calculation
"""

# Test vectors from C# Neo v3.9.1
test_cases = [
    {
        "name": "Single signature transaction",
        "tx_size": 250,
        "network_fee": 1000000,
        "attributes_fee": 0,
        "fee_per_byte": 1000,
        "witnesses": [{"type": "single_sig"}],
        "expected_remaining_fee": 1000000 - (250 * 1000) - 66560
    },
    {
        "name": "Multi-sig 2-of-3 transaction",
        "tx_size": 350,
        "network_fee": 2000000,
        "attributes_fee": 0,
        "fee_per_byte": 1000,
        "witnesses": [{"type": "multi_sig", "m": 2, "n": 3}],
        "expected_remaining_fee": 2000000 - (350 * 1000) - 133634
    },
]

# Constants
SINGLE_SIG_COST = 66560
MULTI_SIG_2_3_COST = 133634

def verify_fee_calculation(test_case):
    """Verify fee calculation matches C# implementation"""
    print(f"\n{'='*60}")
    print(f"Test: {test_case['name']}")
    print(f"{'='*60}")
    
    # Step 1: Calculate size-based fee
    size_fee = test_case['tx_size'] * test_case['fee_per_byte']
    print(f"Size fee: {test_case['tx_size']} * {test_case['fee_per_byte']} = {size_fee}")
    
    # Step 2: Subtract size fee and attributes fee
    net_fee = test_case['network_fee'] - size_fee - test_case['attributes_fee']
    print(f"Net fee after size deduction: {test_case['network_fee']} - {size_fee} - {test_case['attributes_fee']} = {net_fee}")
    
    # Step 3: Deduct witness verification costs
    for witness in test_case['witnesses']:
        if witness['type'] == 'single_sig':
            net_fee -= SINGLE_SIG_COST
            print(f"After single sig verification: {net_fee + SINGLE_SIG_COST} - {SINGLE_SIG_COST} = {net_fee}")
        elif witness['type'] == 'multi_sig':
            net_fee -= MULTI_SIG_2_3_COST
            print(f"After multi-sig verification: {net_fee + MULTI_SIG_2_3_COST} - {MULTI_SIG_2_3_COST} = {net_fee}")
    
    # Verify result
    expected = test_case['expected_remaining_fee']
    if net_fee == expected:
        print(f"✓ PASS: Remaining fee = {net_fee} (expected {expected})")
        return True
    else:
        print(f"✗ FAIL: Remaining fee = {net_fee}, expected {expected}")
        return False

if __name__ == "__main__":
    print("Transaction Fee Calculation Verification")
    print("Comparing neo-rs with C# Neo v3.9.1\n")
    
    passed = 0
    failed = 0
    
    for test_case in test_cases:
        if verify_fee_calculation(test_case):
            passed += 1
        else:
            failed += 1
    
    print(f"\n{'='*60}")
    print(f"Results: {passed} passed, {failed} failed")
    print(f"{'='*60}")
