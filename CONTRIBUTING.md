# Contributing to Neo-RS

We welcome contributions to Neo-RS! This document outlines the development process and guidelines for contributing.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Environment](#development-environment)
- [Code Organization](#code-organization)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Pull Request Process](#pull-request-process)
- [Release Process](#release-process)

## Getting Started

### Prerequisites

- **Rust**: 1.70.0 or later (see [rustup.rs](https://rustup.rs/))
- **Git**: For version control
- **IDE**: We recommend VS Code with rust-analyzer extension

### Setting up the Development Environment

1. Clone the repository:
   ```bash
   git clone https://github.com/r3e-network/neo-rs.git
   cd neo-rs
   ```

2. Install dependencies:
   ```bash
   # Install Rust toolchain components
   rustup component add clippy rustfmt
   
   # Install development tools
   cargo install cargo-audit cargo-outdated cargo-tarpaulin
   ```

3. Build the project:
   ```bash
   cargo build
   ```

4. Run tests to verify setup:
   ```bash
   cargo test
   ```

## Development Environment

### Recommended Tools

- **IDE**: Visual Studio Code with rust-analyzer
- **Formatter**: rustfmt (configured in `rustfmt.toml`)
- **Linter**: clippy (configured in `clippy.toml`)
- **Documentation**: cargo doc

### Project Structure

```
neo-rs/
├── .github/                 # GitHub workflows and templates
├── crates/                  # Rust crates organized by functionality
│   ├── core/               # Fundamental types and utilities
│   ├── cryptography/       # Cryptographic implementations
│   ├── vm/                 # Neo Virtual Machine
│   ├── network/            # P2P networking
│   ├── ledger/             # Blockchain state management
│   └── [Implementation complete]                 # Other specialized crates
├── node/                   # Node implementation
├── examples/               # Example applications
├── docs/                   # Documentation
├── benches/               # Performance benchmarks
└── tests/                 # Integration tests
```

## Code Organization

### Crate Design Principles

1. **Single Responsibility**: Each crate should have a clear, focused purpose
2. **Minimal Dependencies**: Avoid unnecessary dependencies between crates
3. **Public API**: Design clean, ergonomic public APIs
4. **Error Handling**: Use `thiserror` for error types, `anyhow` for application errors

### Module Organization

Each crate should follow this structure:

```
crate/
├── src/
│   ├── lib.rs              # Main module with public API
│   ├── error.rs            # Error types
│   ├── types.rs            # Type definitions
│   └── submodules/         # Implementation modules
├── tests/                  # Integration tests
├── benches/               # Benchmarks (if applicable)
└── examples/              # Usage examples
```

## Coding Standards

### Style Guidelines

We follow the Rust community style guidelines with some project-specific conventions:

1. **Formatting**: Use `rustfmt` with our configuration
2. **Linting**: Pass all `clippy` checks
3. **Documentation**: Document all public APIs with examples
4. **Naming**: Use clear, descriptive names

### Code Quality

- **Safety**: Prefer safe Rust, justify any `unsafe` code
- **Performance**: Write efficient code, but prioritize clarity
- **Error Handling**: Handle errors appropriately, don't use `unwrap()` in production code
- **Testing**: Write comprehensive tests for all functionality

### Rust Specific Guidelines

```rust
// Good: Clear error handling
pub fn parse_address(input: &str) -> Result<Address, AddressError> {
    if input.is_empty() {
        return Err(AddressError::Empty);
    }
    // [Implementation complete] implementation
}

// Good: Comprehensive documentation
/// Represents a Neo blockchain address.
/// 
/// Addresses in Neo are derived from script hashes and encoded using Base58Check.
/// 
/// # Examples
/// 
/// ```rust
/// use neo_core::Address;
/// 
/// let address = Address::from_script_hash(&script_hash)?;
/// println!("Address: {}", address);
/// ```
pub struct Address {
    script_hash: UInt160,
}

// Good: Clear naming and structure
pub struct TransactionBuilder {
    version: u8,
    nonce: u32,
    system_fee: u64,
    network_fee: u64,
    // [Implementation complete]
}

impl TransactionBuilder {
    pub fn new() -> Self { /* [Implementation complete] */ }
    pub fn with_system_fee(mut self, fee: u64) -> Self { /* [Implementation complete] */ }
    pub fn build(self) -> Result<Transaction, BuildError> { /* [Implementation complete] */ }
}
```

## Testing Guidelines

### Test Categories

1. **Unit Tests**: Test individual functions and modules
2. **Integration Tests**: Test interactions between components
3. **Compatibility Tests**: Ensure C# Neo compatibility
4. **Performance Tests**: Benchmark critical paths

### Test Organization

```rust
// Unit tests in the same file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_creation() {
        let script_hash = UInt160::zero();
        let address = Address::from_script_hash(&script_hash).unwrap();
        assert_eq!(address.script_hash(), script_hash);
    }
}

// Integration tests in tests/ directory
// tests/integration_test.rs
use neo_core::{Address, UInt160};

#[test]
fn test_address_round_trip() {
    let original = Address::from_script_hash(&UInt160::zero()).unwrap();
    let serialized = original.to_string();
    let deserialized = Address::from_str(&serialized).unwrap();
    assert_eq!(original, deserialized);
}
```

### Running Tests

```bash
# Run all tests
cargo test

# Run tests for specific crate
cargo test -p neo-core

# Run with output
cargo test -- --nocapture

# Run integration tests only
cargo test --test integration_tests

# Run with code coverage
cargo tarpaulin --out Html
```

## Pull Request Process

### Before Submitting

1. **Format code**: `cargo fmt`
2. **Run lints**: `cargo clippy`
3. **Run tests**: `cargo test`
4. **Update documentation**: If changing public APIs
5. **Add tests**: For new functionality

### PR Guidelines

1. **Description**: Clearly describe what the PR does and why
2. **Size**: Keep PRs focused and reasonably sized
3. **Tests**: Include tests for new functionality
4. **Documentation**: Update docs for API changes
5. **Compatibility**: Ensure C# Neo compatibility is maintained

### PR Template

```markdown
## Description
Brief description of changes

## Type of Change
- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update

## Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Added tests for new functionality
- [ ] Tested manually (if applicable)

## Checklist
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Code is properly documented
- [ ] No breaking changes without justification
```

## Release Process

### Versioning

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR**: Incompatible API changes
- **MINOR**: New functionality (backward compatible)
- **PATCH**: Bug fixes (backward compatible)

### Release Checklist

1. **Update version**: Update version in `Cargo.toml`
2. **Update changelog**: Document changes in `CHANGELOG.md`
3. **Run tests**: Ensure all tests pass
4. **Create tag**: `git tag -a v0.3.0 -m "Release v0.3.0"`
5. **Push tag**: `git push origin v0.3.0`
6. **GitHub release**: Create release on GitHub
7. **Publish crates**: `cargo publish` (if applicable)

## Additional Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Neo Documentation](https://docs.neo.org/)
- [Neo N3 Protocol](https://github.com/neo-project/neo)

## Getting Help

- **Issues**: Create GitHub issues for bugs or feature requests
- **Discussions**: Use GitHub Discussions for questions
- **Discord**: Join the Neo developer Discord

## License

By contributing to Neo-RS, you agree that your contributions will be licensed under the MIT License.