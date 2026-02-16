use std::env;
use std::path::PathBuf;

fn main() {
    if env::var("CARGO_FEATURE_NEOFS_GRPC").is_err() {
        return;
    }

    if env::var("PROTOC").is_err() {
        if let Ok(path) = protoc_bin_vendored::protoc_bin_path() {
            // SAFETY: build scripts are single-process setup steps and set PROTOC before use.
            unsafe { env::set_var("PROTOC", path) };
        }
    }

    let proto_root = PathBuf::from("proto/neofs");
    let object_proto = proto_root.join("object/service.proto");

    tonic_build::configure()
        .build_server(false)
        .compile(&[object_proto], &[proto_root])
        .expect("failed to compile neofs protos");

    println!("cargo:rerun-if-changed=proto/neofs/object/service.proto");
    println!("cargo:rerun-if-changed=proto/neofs/object/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/refs/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/session/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/acl/types.proto");
    println!("cargo:rerun-if-changed=proto/neofs/status/types.proto");
}
