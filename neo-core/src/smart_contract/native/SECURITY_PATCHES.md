# Native Contract Security Patches

This document describes the security patches applied to the native contracts in neo-core.

## Overview

The following security enhancements have been implemented:

1. **Overflow Protection**: All arithmetic operations now use checked arithmetic to prevent integer overflows
2. **Permission Validation**: Enhanced committee and witness checking with stricter validation
3. **Reentrancy Protection**: Guards against reentrancy attacks in token transfers
4. **State Consistency**: Additional validation to ensure state remains consistent after operations

## Files Modified

### 1. `security_fixes.rs` (NEW)
A new module providing security utilities:
- `SecurityContext`: Tracks reentrancy guards and operation contexts
- `SafeArithmetic`: Checked arithmetic operations for BigInt
- `PermissionValidator`: Enhanced permission checking utilities
- `StateValidator`: State consistency validation helpers

### 2. `gas_token.rs`
- Added overflow checks in mint/burn operations
- Added reentrancy protection for transfer operations
- Added state consistency validation after balance updates

### 3. `neo_token/nep17.rs`
- Added overflow checks in balance updates
- Added reentrancy protection for transfer
- Enhanced validation for account state changes

### 4. `neo_token/governance.rs`
- Added overflow checks in reward calculations
- Added validation for committee operations
- Added state consistency checks for voting operations

### 5. `contract_management/deploy.rs`
- Enhanced validation for contract deployment
- Added overflow check for deployment fee calculation

### 6. `policy_contract/setters.rs`
- Added permission validation helpers
- Enhanced parameter validation

## Security Considerations

### Integer Overflow
All arithmetic operations that could potentially overflow now use checked operations:
- Balance additions/subtractions
- Reward calculations
- Fee calculations
- Vote counting

### Reentrancy
The following patterns protect against reentrancy:
- Reentrancy guards on state-changing operations
- Checks-effects-interactions pattern
- Snapshot isolation for read operations

### Permission Validation
Enhanced permission checking includes:
- Committee signature validation
- Witness verification
- Native contract caller verification
- Call flags validation

### State Consistency
State validation ensures:
- Total supply never goes negative
- Account balances are consistent
- Vote counts match actual votes
- Committee state is valid

## Testing

Security tests have been added to verify:
1. Overflow protection works correctly
2. Reentrancy attempts are blocked
3. Permission checks prevent unauthorized access
4. State remains consistent after operations

Run tests with:
```bash
cargo test -p neo-core security
cargo test -p neo-core overflow
cargo test -p neo-core permission
```
