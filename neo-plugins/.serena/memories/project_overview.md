# neo-plugins overview
- Purpose: Rust port of Neo blockchain plugins collection matching C# plugins.
- Tech stack: Rust 2021; depends on sibling Neo crates (neo-core, neo-vm, neo-io, etc.), async runtime tokio, serialization serde, DB backends rusqlite and RocksDB, warp for HTTP APIs.
- Structure: crate root exports plugin modules. Key dirs: src/application_logs, src/dbft_plugin, src/rocksdb_store, src/rpc_server, src/sqlite_wallet, src/tokens_tracker; root lib registers available plugins.
- Platform: Linux, cargo-managed Rust project.