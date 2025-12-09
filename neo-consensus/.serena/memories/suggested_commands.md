# Suggested commands
- Build crate: `cargo build -p neo-consensus`
- Run tests for this crate: `cargo test -p neo-consensus`
- Lint with clippy (all targets/features): `cargo clippy -p neo-consensus --all-targets --all-features -- -D warnings`
- Format with rustfmt: `cargo fmt`
- Generate docs for this crate: `cargo doc -p neo-consensus --no-deps --open`
- Workspace root (../) is the recommended working directory for commands so shared deps resolve correctly.
