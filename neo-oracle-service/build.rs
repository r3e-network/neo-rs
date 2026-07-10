//! Build script for optional NeoFS gRPC client bindings.

use std::env;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    if env::var("CARGO_FEATURE_NEOFS_GRPC").is_err() {
        return Ok(());
    }

    if env::var("PROTOC").is_err() {
        if let Ok(path) = protoc_bin_vendored::protoc_bin_path() {
            set_env_var("PROTOC", path);
        }
    }

    let proto_root = PathBuf::from("proto/neofs");
    let object_proto = proto_root.join("object/service.proto");

    tonic_build::configure()
        .build_server(false)
        .compile(&[object_proto], &[proto_root])
        .map_err(|err| format!("compile NeoFS protobufs: {err}"))?;

    println!("cargo:rerun-if-changed=proto/neofs/object/service.proto");
    println!("cargo:rerun-if-changed=proto/neofs/object/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/refs/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/session/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/acl/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/status/types.proto");
    Ok(())
}

fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
    // SAFETY: Build scripts run as short-lived single-purpose processes. This
    // mutation happens before invoking prost/tonic code that reads PROTOC.
    // Rationale: Rust 2024 marks environment mutation unsafe; this build script
    // performs it before any concurrent work and keeps the generated-code tool
    // path deterministic.
    #[allow(unused_unsafe, unsafe_code)]
    unsafe {
        env::set_var(key, value);
    }
}
