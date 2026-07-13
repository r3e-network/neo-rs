# Contributing Guide

Thanks for helping improve the Neo Rust node. Please follow these guidelines for pull requests:

## Development setup
- Use the stable Rust toolchain (`rustup default stable`).
- Install a C/C++ toolchain plus Clang/libclang for the bundled MDBX bindings.

## Checks before opening a PR
- Format: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Tests: `cargo test --workspace`
- If you touch Docker/compose, ensure `docker build` and `docker compose config` still succeed.
- For docs-only changes, still run `cargo fmt` to keep CI green.

## Code style
- Prefer small, focused commits with descriptive messages.
- Add targeted comments when behaviour is non-obvious; avoid restating the code.
- Keep configuration defaults sensible for production where applicable (e.g., MDBX backend, non-root containers).
- Follow the high-level flow guidance in `docs/coding-design-architecture-guidance.md`: top-level code should read as domain intent, with protocol/storage/RPC/runtime details hidden behind lower-layer operations.

## Security
- Do **not** open public issues for suspected vulnerabilities. Follow the process in `SECURITY.md`.

## Release notes
- Update `CHANGELOG.md` when behaviour changes, tooling is added, or external interfaces (CLI flags, configs, Docker) are modified.

## Communication
- Be clear about any assumptions and open questions in your PR description.
- Link related issues and describe user-facing impact.
