# Protocol Divergences - Neo N3 v3.9.1

## Status: IDENTIFIED
Last Updated: 2026-03-23

## CRITICAL Priority (Must Fix Before Production)

### CRITICAL-001: Block Validation
- **Component**: Block processing
- **Status**: NEEDS_VERIFICATION
- **Action**: Create test vectors and verify

### CRITICAL-002: State Root Calculation
- **Component**: State trie
- **Status**: NEEDS_VERIFICATION
- **Action**: Add compliance tests

### CRITICAL-003: Transaction Signature Verification
- **Component**: Transaction validation
- **Status**: NEEDS_VERIFICATION
- **Action**: Test signature verification

### CRITICAL-004: Transaction Fee Calculation
- **Component**: Transaction validation
- **Status**: NEEDS_VERIFICATION
- **Action**: Verify fee calculations

### CRITICAL-005: Consensus Message Handling
- **Component**: dBFT 2.0 consensus
- **Status**: NEEDS_VERIFICATION
- **Action**: Test consensus messages

### CRITICAL-006: View Change Logic
- **Component**: dBFT 2.0 consensus
- **Status**: NEEDS_VERIFICATION
- **Action**: Test view change scenarios

### CRITICAL-007: VM Instruction Execution
- **Component**: NeoVM
- **Status**: NEEDS_VERIFICATION
- **Action**: Test all opcodes

### CRITICAL-008: Syscall Implementations
- **Component**: Smart contract execution
- **Status**: NEEDS_VERIFICATION
- **Action**: Verify all syscalls

### CRITICAL-009: NeoToken Contract
- **Component**: Native contracts
- **Status**: NEEDS_VERIFICATION
- **Action**: Test NeoToken behavior

### CRITICAL-010: GasToken Contract
- **Component**: Native contracts
- **Status**: NEEDS_VERIFICATION
- **Action**: Test GasToken behavior
