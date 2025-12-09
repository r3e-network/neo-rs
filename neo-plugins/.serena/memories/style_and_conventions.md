# Style and conventions
- Rust 2021 edition; modules follow idiomatic Rust module structure.
- Uses doc comments (`//!` and `///`) for module and function docs; prefer clear safety notes when using unsafe.
- Concurrency via `Arc`, `Mutex` (parking_lot); avoid unsafe unless necessary.
- Formatting: use `cargo fmt` (rustfmt); lint with `cargo clippy` when appropriate.
- Error handling: uses `Result` from neo-core extensions and `thiserror` for custom errors; prefer `?` for propagation.