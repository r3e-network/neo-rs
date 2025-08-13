# Contributing to Neo-RS

We welcome contributions to Neo-RS! This document provides guidelines for contributing to the project.

## Code of Conduct

By participating in this project, you agree to abide by our Code of Conduct. Please treat all contributors with respect and create a welcoming environment for everyone.

## Getting Started

1. Fork the repository on GitHub
2. Clone your fork locally:
   ```bash
   git clone https://github.com/your-username/neo-rs.git
   cd neo-rs
   ```
3. Set up the development environment:
   ```bash
   # Install Rust if you haven't already
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Install system dependencies
   # Ubuntu/Debian:
   sudo apt-get install build-essential clang librocksdb-dev
   
   # macOS:
   brew install rocksdb
   ```
4. Build the project:
   ```bash
   cargo build --workspace
   ```
5. Run the tests:
   ```bash
   cargo test --workspace
   ```

## Development Guidelines

### Code Style

- Follow Rust standard formatting: `cargo fmt`
- Address all clippy warnings: `cargo clippy --workspace`
- Use meaningful variable and function names
- Write comprehensive documentation for public APIs
- Keep functions focused and small when possible

### Testing

- Write tests for all new functionality
- Ensure existing tests continue to pass
- Aim for high test coverage
- Include both unit tests and integration tests where appropriate
- Test error conditions and edge cases

### Documentation

- Document all public APIs with rustdoc comments
- Include examples in documentation where helpful
- Update the README.md if adding significant features
- Keep CHANGELOG.md updated

### Performance

- Profile performance-critical code changes
- Avoid unnecessary allocations in hot paths
- Use benchmarks to validate performance improvements
- Consider memory usage implications

## Submitting Changes

### Pull Request Process

1. Create a feature branch from main:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make your changes following the guidelines above

3. Commit your changes with clear, descriptive messages:
   ```bash
   git commit -m "feat: add new transaction validation logic
   
   - Implement additional validation checks for transaction signatures
   - Add comprehensive test coverage for edge cases
   - Update documentation with new validation rules"
   ```

4. Push your branch:
   ```bash
   git push origin feature/your-feature-name
   ```

5. Open a Pull Request on GitHub with:
   - Clear title describing the change
   - Detailed description of what was changed and why
   - Reference to any related issues
   - Screenshots or examples if applicable

### Commit Message Guidelines

Use conventional commit format:
- `feat:` for new features
- `fix:` for bug fixes
- `docs:` for documentation changes
- `style:` for formatting changes
- `refactor:` for code restructuring
- `test:` for adding tests
- `chore:` for maintenance tasks

### Pull Request Requirements

Before submitting a PR, ensure:
- [ ] All tests pass: `cargo test --workspace`
- [ ] Code is formatted: `cargo fmt`
- [ ] No clippy warnings: `cargo clippy --workspace`
- [ ] Documentation is updated if needed
- [ ] CHANGELOG.md is updated for significant changes

## Types of Contributions

### Bug Reports

When reporting bugs, please include:
- Steps to reproduce the issue
- Expected vs actual behavior
- Environment details (OS, Rust version, etc.)
- Relevant error messages or logs

### Feature Requests

For new features:
- Describe the use case and motivation
- Propose an API design if applicable
- Consider backwards compatibility
- Discuss performance implications

### Code Contributions

We welcome:
- Bug fixes
- Performance improvements
- New features
- Documentation improvements
- Test coverage improvements
- Code quality improvements

## Development Areas

### High Priority
- Performance optimizations
- Bug fixes and stability improvements
- Test coverage expansion
- Documentation improvements

### Medium Priority
- New features aligned with Neo protocol
- Developer tooling improvements
- Additional storage backends
- Enhanced monitoring

### Low Priority
- Code refactoring for maintainability
- Additional examples and tutorials
- Performance benchmarking
- Development workflow improvements

## Getting Help

- Open an issue for bugs or feature requests
- Start a discussion for questions or ideas
- Join our Discord for real-time discussion
- Check existing documentation and issues first

## Recognition

Contributors will be recognized in:
- Git commit history
- Release notes for significant contributions
- Project documentation
- Annual contributor acknowledgments

Thank you for contributing to Neo-RS! 🦀