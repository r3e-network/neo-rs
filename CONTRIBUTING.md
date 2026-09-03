# Contributing Guide

Thanks for helping improve the Neo Rust node. Please follow these guidelines for pull requests:

## Development setup
- Use the stable Rust toolchain (`rustup default stable`).
- Install RocksDB development headers on your platform (e.g., `sudo apt-get install librocksdb-dev` on Debian/Ubuntu) to build and run tests.

## Checks before opening a PR
- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Tests: `cargo test --workspace`
- If you touch Docker/compose, ensure `docker build` and `docker compose config` still succeed.
- For docs-only changes, still run `cargo fmt` to keep CI green.

## Code style
- Prefer small, focused commits with descriptive messages.
- Add targeted comments when behaviour is non-obvious; avoid restating the code.
- Keep configuration defaults sensible for production where applicable (e.g., rocksdb backend, non-root containers).

## Security
- Do **not** open public issues for suspected vulnerabilities. Follow the process in `SECURITY.md`.

## Release notes
- Update `CHANGELOG.md` when behaviour changes, tooling is added, or external interfaces (CLI flags, configs, Docker) are modified.

## Communication
- Be clear about any assumptions and open questions in your PR description.
- Link related issues and describe user-facing impact.
