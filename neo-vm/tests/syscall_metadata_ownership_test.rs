//! Syscall metadata ownership tests.
//!
//! The VM owns generic syscall dispatch machinery only. Neo host-service
//! metadata (canonical names, gas prices, required call flags) is registered by
//! the application-engine layer, which is the single authority for it. These
//! tests pin that contract so a hardcoded VM-side table cannot reappear and
//! silently disagree with the registration.

use neo_primitives::CallFlags;
use neo_vm::interop_hash;
use neo_vm::{ExecutionEngine, InteropService};

/// `InteropService::new` must start empty: the VM ships no built-in Neo table.
#[test]
fn interop_service_has_no_builtin_syscalls() {
    let service = InteropService::new();
    assert!(service.is_empty());
    assert_eq!(service.len(), 0);

    // A canonical Neo syscall name is unknown until someone registers it.
    assert!(service.get_method(b"System.Storage.Get").is_none());
    assert!(service.get_method(b"System.Runtime.Platform").is_none());
}

/// Registration is the only source of syscall metadata.
#[test]
fn registration_is_the_single_metadata_source() {
    let mut service = InteropService::new();
    let hash = service
        .register_host_descriptor("System.Storage.Put", 1 << 15, CallFlags::WRITE_STATES)
        .expect("register storage put");

    assert_eq!(hash, interop_hash("System.Storage.Put"));

    let descriptor = service
        .get_method(b"System.Storage.Put")
        .expect("descriptor must be registered");
    assert_eq!(descriptor.name, "System.Storage.Put");
    assert_eq!(descriptor.price, 1 << 15);
    assert_eq!(descriptor.required_call_flags, CallFlags::WRITE_STATES);
    assert!(descriptor.handler.is_none());
}

/// An unregistered hash is rejected by dispatch instead of falling back to a
/// hardcoded table.
#[test]
fn unregistered_syscall_is_rejected() {
    let hash = interop_hash("System.Storage.Get");

    // No interop service installed at all.
    let mut bare = ExecutionEngine::new(None);
    bare.on_syscall(hash)
        .expect_err("syscall without an interop service must not dispatch");

    // An installed but empty service: no built-in fallback exists, so even a
    // canonical Neo name is unknown until the application engine registers it.
    let mut engine = ExecutionEngine::new(None);
    engine.set_interop_service(InteropService::new());
    let error = engine
        .on_syscall(hash)
        .expect_err("unregistered syscall must not dispatch");
    assert!(
        error.to_string().contains("not registered"),
        "unexpected error for unregistered syscall: {error}"
    );

    // Registering it makes the very same hash dispatchable.
    let mut service = InteropService::new();
    service
        .register_host_descriptor("System.Storage.Get", 1 << 15, CallFlags::READ_STATES)
        .expect("register storage get");
    engine.set_interop_service(service);
    assert!(engine.interop_service_mut().is_some());
    // Without an `InteropHost` the host-owned call cannot run, but it is now
    // routed to the host instead of being rejected as unknown.
    let routed = engine
        .on_syscall(hash)
        .expect_err("host-owned syscall needs a host");
    assert!(
        routed.to_string().contains("requires an interop host"),
        "registration must route to the host: {routed}"
    );
}
