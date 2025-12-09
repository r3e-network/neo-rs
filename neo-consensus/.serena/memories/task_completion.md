# Task completion checklist
- From workspace root, ensure formatting: `cargo fmt`
- Lint for this crate (or workspace if needed): `cargo clippy -p neo-consensus --all-targets --all-features -- -D warnings`
- Run tests for this crate: `cargo test -p neo-consensus`
- If adding public APIs, consider `cargo doc -p neo-consensus --no-deps` to catch doc errors.
- Summarize changes and note any unrun tests or limitations when handing off.
