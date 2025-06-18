# Wallets Module Conversion

This document details the conversion of the Neo N3 C# Wallets module (@neo-sharp/) to Rust (@neo-rs/).

## 🟡 CONVERSION STATUS: IN PROGRESS

The Wallets module conversion has been started with basic structure created, but needs API compatibility fixes.

- **Compilation Errors:** 33 errors to fix
- **Basic Structure:** ✅ Complete
- **C# API Compatibility:** 🔴 Needs work
- **Tests:** 🔴 Not implemented yet

## File Mappings (C# @neo-sharp/ → Rust @neo-rs/)

| C# File | Rust File | Implementation Status |
|---------|-----------|------------------------|
| `Wallet.cs` | `wallet.rs` | 🟡 **Basic structure** |
| `WalletAccount.cs` | `wallet_account.rs` | 🟡 **Basic structure** |
| `KeyPair.cs` | `key_pair.rs` | 🟡 **Basic structure** |
| `NEP6/NEP6Wallet.cs` | `nep6.rs` | 🟡 **Basic structure** |
| `NEP6/NEP6Account.cs` | `nep6.rs` (Nep6Account) | 🟡 **Basic structure** |
| `NEP6/NEP6Contract.cs` | `nep6.rs` (Nep6Contract) | 🟡 **Basic structure** |
| `NEP6/ScryptParameters.cs` | `scrypt_parameters.rs` | ✅ **Complete** |
| `Contract.cs` | `contract.rs` | 🟡 **Basic structure** |
| `WalletFactory.cs` | `wallet_factory.rs` | 🟡 **Basic structure** |

## C# to Rust Conversion Details

### Key Architectural Differences

**C# Structure (@neo-sharp/):**
- `Wallet` abstract base class with virtual methods
- `WalletAccount` abstract base class with inheritance
- `NEP6Wallet : Wallet` (inheritance)
- `NEP6Account : WalletAccount` (inheritance)
- Static factory methods and properties
- Exception-based error handling

**Rust Structure (@neo-rs/):**
- `Wallet` trait with async methods
- `WalletAccount` trait with async methods
- `Nep6Wallet` struct implementing `Wallet` trait
- `Nep6Account` struct implementing `WalletAccount` trait
- Factory pattern with trait objects
- Result-based error handling

### Conversion Patterns Applied

1. **Abstract Classes → Traits**
   - C# abstract classes converted to Rust traits
   - Virtual methods become trait methods

2. **Inheritance → Composition + Traits**
   - C# inheritance chains converted to trait implementations
   - Composition used for code reuse

3. **Static Methods → Associated Functions**
   - C# static methods → Rust associated functions
   - Factory pattern for wallet creation

4. **Exception Handling → Result Types**
   - C# exceptions → Rust `Result<T, Error>` pattern
   - Async error propagation with `?` operator

## Current Issues to Fix

### 1. API Compatibility Issues (33 errors)

**Missing Cryptography APIs:**
- `neo_cryptography::hash` module missing
- `neo_cryptography::ECC` and `ECDsa` missing
- Need to implement proper crypto APIs

**Missing Core APIs:**
- `UInt160::from_script()` method missing
- `UInt160::to_address()` method missing
- `Witness` constructor issues

**Library API Mismatches:**
- `bs58` crate API differences (`.with_check()` method)
- `aes` crate API differences (encryption/decryption methods)
- `scrypt` crate API differences

### 2. Trait Implementation Issues

**Nep6Account needs to implement WalletAccount:**
```rust
#[async_trait]
impl WalletAccount for Nep6Account {
    // Implement all trait methods
}
```

**Missing methods:**
- `Nep6Account::from_file()` and `to_file()` methods
- `KeyPair::get_public_key_point()` method

### 3. Type System Issues

**Async trait compatibility:**
- Some methods return wrong types for async traits
- Need proper `Result<T, Error>` returns

## Implementation Plan

### Phase 1: Fix Core Dependencies ✅ (Current)
1. ✅ Create basic module structure
2. 🔄 Fix cryptography module APIs
3. 🔄 Fix core module APIs (UInt160, Witness)
4. 🔄 Update library dependencies

### Phase 2: Complete Trait Implementations
1. Implement `WalletAccount` for `Nep6Account`
2. Add missing methods to `KeyPair`
3. Fix async trait compatibility issues
4. Complete NEP-6 file format handling

### Phase 3: Testing and Validation
1. Convert C# unit tests to Rust
2. Test NEP-6 wallet creation and loading
3. Test key import/export (WIF, NEP-2)
4. Test transaction signing

### Phase 4: Advanced Features
1. Multi-signature wallet support
2. Hardware wallet integration
3. Watch-only account support
4. Wallet migration tools

## Key Features Implemented

### ✅ Basic Structure
- Wallet trait with async methods
- WalletAccount trait with async methods
- NEP-6 wallet file format structures
- Scrypt parameters for encryption
- Key pair generation and management

### 🔄 In Progress
- NEP-6 wallet implementation
- Contract creation and management
- Key import/export functionality
- Wallet factory pattern

### 🔴 Not Started
- Unit tests
- Integration with blockchain
- Hardware wallet support
- Advanced multi-sig features

## Testing Strategy

1. **Unit Tests** - Test individual components
   - Key pair generation and signing
   - Scrypt parameter validation
   - Contract creation
   - NEP-6 file format serialization

2. **Integration Tests** - Test wallet operations
   - Wallet creation and loading
   - Account management
   - Transaction signing
   - Key import/export

3. **Compatibility Tests** - Ensure C# compatibility
   - Load C# NEP-6 wallets in Rust
   - Verify identical key derivation
   - Test cross-platform compatibility

## Next Steps

1. **Fix compilation errors** - Address the 33 compilation errors
2. **Complete trait implementations** - Ensure all traits are properly implemented
3. **Add missing APIs** - Implement missing methods in core and crypto modules
4. **Write tests** - Convert C# tests and add Rust-specific tests
5. **Integration testing** - Test with other Neo-Rust modules

The Wallets module represents a critical component for Neo user interaction and requires careful attention to security and C# compatibility.
