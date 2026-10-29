# Contributing to neo-rs

Thanks for helping improve the Neo Rust node implementation. This guide will help you get started with contributing to the project.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Workflow](#workflow)
- [Code Standards](#code-standards)
- [Testing](#testing)
- [Documentation](#documentation)
- [Pull Request Process](#pull-request-process)
- [Communication](#communication)
- [Security](#security)

## Getting Started

### Finding Work

- **Good First Issues**: Look for issues labeled `good-first-issue` in the GitHub issue tracker
- **Help Wanted**: Issues labeled `help-wanted` are ready for community contributions
- **Documentation**: Improvements to docs, examples, and guides are always welcome
- **Testing**: Adding test coverage, especially for edge cases and protocol compliance

### Before You Start

1. Read the [Architecture Guide](ARCHITECTURE.md) to understand the codebase structure
2. Review the [Style Guide](docs/STYLE.md) for code organization and naming conventions
3. Check existing issues and PRs to avoid duplicate work
4. For large features, open an issue first to discuss the approach

## Development Setup

### Prerequisites

- **Rust**: 1.99.1, pinned by `rust-toolchain.toml`. Install [rustup](https://rustup.rs/);
  it selects the pinned toolchain automatically inside the repository. See
  [docs/TOOLCHAIN.md](docs/TOOLCHAIN.md) for the toolchain and lint policy.

- **RocksDB**: Development headers required for storage backend:
  ```bash
  # Debian/Ubuntu
  sudo apt-get install librocksdb-dev

  # macOS
  brew install rocksdb

  # Windows
  # See https://github.com/rust-rocksdb/rust-rocksdb#windows
  ```

- **Git**: For version control

### Clone and Build

```bash
# Fork the repository on GitHub, then clone your fork
git clone https://github.com/YOUR_USERNAME/neo-rs.git
cd neo-rs

# Add upstream remote
git remote add upstream https://github.com/r3e-network/neo-rs.git

# Build with default features (faster for development)
cargo build

# Build with full features (includes RocksDB backend)
cargo build --features full

# Build release version
cargo build --release --features full
```

### Build Features

The project uses Cargo features to enable optional functionality:

- `full`: Enables RocksDB storage backend (required for production configs)
- `tee`: Trusted Execution Environment support
- `hsm`: Hardware Security Module support
- `runtime`: VM runtime features (used in neo-core tests)
- `server`: RPC server features (used in neo-rpc tests)

For development, the default feature set is usually sufficient. Use `--features full` when testing with production configurations.

## Workflow

### 1. Create a Branch

Always create a feature branch from the latest `main`:

```bash
# Update your local main
git checkout main
git pull upstream main

# Create a feature branch
git checkout -b feature/your-feature-name
# or for bug fixes
git checkout -b fix/issue-description
```

Branch naming conventions:
- `feature/description` - New features
- `fix/description` - Bug fixes
- `docs/description` - Documentation changes
- `refactor/description` - Code refactoring
- `test/description` - Test additions or improvements

### 2. Make Changes

- Keep commits focused and atomic
- Write clear commit messages (see [Commit Messages](#commit-messages))
- Add tests for new functionality
- Update documentation as needed

### 3. Commit Messages

Follow this format for commit messages:

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types**:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, no logic change)
- `refactor`: Code refactoring
- `test`: Adding or updating tests
- `chore`: Maintenance tasks, dependency updates

**Examples**:
```
feat(rpc): add rate limiting to JSON-RPC endpoints

Implement token bucket rate limiting using the governor crate.
Default limit is 100 requests per minute per IP.

Closes #123

---

fix(consensus): prevent duplicate change view messages

Add deduplication check before broadcasting ChangeView.
Resolves issue where nodes would spam the network during
view changes.

Fixes #456
```

### 4. Keep Your Branch Updated

Regularly sync with upstream to avoid conflicts:

```bash
git fetch upstream
git rebase upstream/main

# If conflicts occur, resolve them and continue
git add .
git rebase --continue
```

### 5. Run Pre-PR Checks

Before opening a PR, ensure all checks pass locally:

```bash
# Format code
cargo fmt --all

# Run linter
cargo clippy --workspace --all-targets -- -D warnings

# Run tests
cargo test --workspace

# Run tests with runtime features (for neo-core)
cargo test -p neo-core --features runtime

# Run tests with server features (for neo-rpc)
cargo test -p neo-rpc --features server

# Check documentation builds
cargo doc --workspace --no-deps

# If you modified Docker files
docker build -t neo-rs-test .
docker compose config
```

### 6. Protocol Compliance (if applicable)

If your changes affect protocol behavior, run compatibility checks:

```bash
# Validate against C# v3.10.1 reference
bash scripts/validate-v3101-consistency.sh --network all

# Run protocol vectors
cargo test --workspace --test protocol_vectors
```

## Code Standards

### Style

- Use `cargo fmt` for consistent formatting
- Follow Rust naming conventions (snake_case for functions/variables, PascalCase for types)
- Keep line length under 100 characters where practical
- Prefer explicit types when they improve readability

### Best Practices

- **Error Handling**: Use `Result` types, avoid panics in library code
- **Documentation**: Public APIs must have doc comments with examples
- **Safety**: Avoid `unsafe` unless necessary; document safety invariants
- **Performance**: Profile before optimizing; document performance-critical sections
- **Dependencies**: Minimize new dependencies; prefer workspace-shared versions

### Architecture Guidelines

- **Layered Architecture**: Respect dependency boundaries (see [ARCHITECTURE.md](ARCHITECTURE.md))
- **Foundation Layer** (neo-primitives, neo-crypto, neo-storage, neo-io, neo-json, neo-config): No cross-module dependencies
- **Core Layer** (neo-vm, neo-core, neo-p2p, neo-consensus): Can depend on foundation layer
- **Application Layer** (neo-node, neo-rpc): Can depend on all lower layers

**Never introduce circular dependencies between crates.**

### Code Comments

- Add comments for non-obvious behavior, algorithms, or protocol requirements
- Reference C# implementation when maintaining compatibility: `// Matches C# Neo.csproj: ClassName.Method`
- Document assumptions and invariants
- Avoid restating what the code already says

## Testing

### Running Tests

```bash
# All workspace tests
cargo test --workspace

# Specific crate
cargo test -p neo-core

# Specific test
cargo test -p neo-vm test_name

# With output
cargo test -- --nocapture

# Integration tests
cargo test --workspace --test integration_test_name
```

### Test Categories

1. **Unit Tests**: Test individual functions and modules
   - Located in `#[cfg(test)] mod tests` within source files
   - Focus on edge cases and error conditions

2. **Integration Tests**: Test crate interactions
   - Located in `tests/` directory
   - Test public API behavior

3. **Protocol Tests**: Ensure C# compatibility
   - Located in `tests/protocol_vectors/`
   - Validate serialization, gas costs, execution results

4. **Compatibility Tests**: Run in CI
   - `.github/workflows/compatibility-v3101.yml`
   - Compare behavior against live C# nodes

### Writing Tests

- Test new features and bug fixes
- Include both success and failure cases
- Use descriptive test names: `test_transfer_with_insufficient_balance_fails`
- Use `#[should_panic]` sparingly; prefer `assert!(result.is_err())`

Example:
```rust
#[test]
fn test_uint256_from_hex_valid() {
    let hex = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    let result = UInt256::from_hex(hex);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().to_string(), hex);
}

#[test]
fn test_uint256_from_hex_invalid_length() {
    let hex = "0x1234";
    let result = UInt256::from_hex(hex);
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ParseError::InvalidLength));
}
```

## Documentation

### Code Documentation

All public APIs require documentation:

```rust
/// Converts a hexadecimal string to a UInt256.
///
/// # Arguments
///
/// * `hex` - A hexadecimal string with optional "0x" prefix
///
/// # Returns
///
/// Returns `Ok(UInt256)` on success, or `Err(ParseError)` if the input is invalid.
///
/// # Examples
///
/// ```
/// use neo_primitives::UInt256;
///
/// let hash = UInt256::from_hex("0x1234...").unwrap();
/// ```
///
/// # Errors
///
/// Returns `ParseError::InvalidLength` if the hex string is not 64 characters.
/// Returns `ParseError::InvalidCharacter` if the string contains non-hex characters.
pub fn from_hex(hex: &str) -> Result<Self, ParseError> {
    // Implementation
}
```

### Documentation Files

When adding features that affect users, update:

- **README.md**: For user-facing features, configuration changes
- **CHANGELOG.md**: For all behavior changes, new features, bug fixes
- **ARCHITECTURE.md**: For structural changes
- **docs/**: For detailed guides (RPC_USAGE.md, DEPLOYMENT.md, OPERATIONS.md, etc.)

### Examples

Add examples to `examples/` directory for common use cases:

```rust
// neo-core/examples/verify_transaction.rs
use neo_core::*;

fn main() {
    // Example code demonstrating transaction verification
}
```

## Pull Request Process

### Before Opening a PR

1. ✅ Run all pre-PR checks (formatting, linting, tests)
2. ✅ Update CHANGELOG.md with your changes
3. ✅ Update documentation if behavior changes
4. ✅ Ensure your branch is up to date with main
5. ✅ Write a clear PR description

### PR Template

Use this template for your PR description:

```markdown
## Description

Brief description of what this PR does and why.

## Changes

- List of specific changes made
- Another change
- Third change

## Testing

- [ ] Unit tests added/updated
- [ ] Integration tests pass
- [ ] Protocol compatibility verified (if applicable)
- [ ] Manual testing performed

## Checklist

- [ ] Code formatted with `cargo fmt`
- [ ] Lints pass with `cargo clippy`
- [ ] Tests pass with `cargo test --workspace`
- [ ] Documentation updated
- [ ] CHANGELOG.md updated

## Related Issues

Closes #123
Relates to #456

## Breaking Changes

None / List any breaking changes and migration path
```

### PR Guidelines

- **Title**: Clear and concise (e.g., "feat(rpc): add rate limiting to endpoints")
- **Size**: Keep PRs focused; break large changes into smaller PRs
- **Description**: Explain what, why, and how
- **Testing**: Describe what testing was done
- **Screenshots**: Include for UI changes (CLI output, logs, etc.)
- **Breaking Changes**: Clearly document any breaking changes

### Review Process

1. **Automated Checks**: CI must pass (formatting, linting, tests, compatibility)
2. **Code Review**: Maintainers will review your code
3. **Feedback**: Address review comments promptly
4. **Approval**: At least one maintainer approval required
5. **Merge**: Maintainers will merge approved PRs

### After Merge

- Delete your feature branch
- Update your local main: `git pull upstream main`
- Close any related issues if not auto-closed

## Communication

### Channels

- **GitHub Issues**: Bug reports, feature requests, questions
- **Pull Requests**: Code review and discussion
- **Discussions**: General questions and community discussions

### Issue Templates

Use the appropriate template when creating issues:

- **Bug Report**: For reporting bugs
- **Feature Request**: For suggesting new features
- **Question**: For asking questions

### PR Communication

- **Be Clear**: Clearly describe assumptions and open questions
- **Link Issues**: Reference related issues and PRs
- **User Impact**: Describe how changes affect users
- **Be Responsive**: Respond to review comments in a timely manner
- **Be Respectful**: Follow the code of conduct

## Security

### Reporting Vulnerabilities

**Do NOT open public issues for security vulnerabilities.**

Follow the process in [SECURITY.md](SECURITY.md):

- Email: `security@r3e.network`
- Include clear description, reproduction steps, and proof-of-concept
- We will acknowledge within 5 business days

### Security Best Practices

- Never commit secrets, keys, or credentials
- Use `zeroize` for sensitive data in memory
- Validate all external inputs
- Follow Rust safety guidelines
- Run security checks: `bash scripts/security-check.sh`

## Additional Resources

### Documentation

- [Architecture Guide](ARCHITECTURE.md) - System architecture and design
- [Style Guide](docs/STYLE.md) - Code organization and conventions
- [RPC Usage](docs/RPC_USAGE.md) - JSON-RPC API documentation
- [Deployment Guide](docs/DEPLOYMENT.md) - Production deployment
- [Operations Guide](docs/OPERATIONS.md) - Maintenance and operations

### Scripts

Helpful scripts in `scripts/`:

- `validate-v3101-consistency.sh` - Protocol compatibility validation
- `security-check.sh` - Security auditing
- `backup-rocksdb.sh` - Database backup
- `mainnet-full-validation.sh` - Full MainNet validation

### CI Workflows

Located in `.github/workflows/`:

- `ci.yml` - Main CI pipeline (formatting, linting, tests)
- `compatibility-v3101.yml` - Protocol compliance checks
- `fuzz.yml` - Fuzzing tests
- `formal-verification.yml` - Formal verification

### Key Concepts

- **Neo N3**: Neo blockchain version 3
- **C# Parity**: Byte-for-byte compatibility with official C# implementation
- **Protocol Version**: Currently targeting v3.10.1
- **Native Contracts**: Built-in smart contracts (GasToken, NeoToken, etc.)
- **dBFT**: Delegated Byzantine Fault Tolerance consensus

## Release Notes

When your PR is merged, it should be documented in `CHANGELOG.md` under the "Unreleased" section. Changes are categorized as:

- **Added**: New features
- **Changed**: Changes to existing functionality
- **Deprecated**: Features that will be removed
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security improvements

## License

By contributing to neo-rs, you agree that your contributions will be licensed under the MIT License.

---

Thank you for contributing to neo-rs! Your efforts help build a robust Rust implementation of the Neo blockchain.
