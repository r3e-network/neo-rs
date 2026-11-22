use super::registration::default_interops;
use super::*;

#[test]
fn faun_and_pre_faun_default_registries_are_isolated() {
    let pre_faun = default_interops(false, false).expect("pre-Faun registry");
    let faun = default_interops(true, false).expect("Faun registry");
    let common = b"System.Storage.Get";
    assert_eq!(
        pre_faun.service.get_price(common),
        faun.service.get_price(common)
    );

    for name in [
        &b"System.Storage.Local.Get"[..],
        &b"System.Storage.Local.Put"[..],
        &b"System.Storage.Local.Delete"[..],
        &b"System.Storage.Local.Find"[..],
    ] {
        assert!(pre_faun.service.get_method(name).is_none());
        assert!(faun.service.get_method(name).is_some());
    }
    assert_eq!(pre_faun.service.len() + 4, faun.service.len());
}

/// The application engine is the single owner of Neo host-service metadata.
/// These tests pin that ownership: names, base prices and required call
/// flags are asserted on the registered descriptors themselves, so a second
/// copy elsewhere (the VM used to ship one) cannot silently disagree again.
fn descriptor<'a>(
    service: &'a neo_vm::InteropService,
    name: &str,
    faun: bool,
) -> &'a neo_vm::interop_service::VmInteropDescriptor {
    service
        .get_method(name.as_bytes())
        .unwrap_or_else(|| panic!("{name} must be registered (faun={faun})"))
}

/// Storage permission bits are per operation, not per namespace.
///
/// Reads (`Get`, `Find`, and the context helpers) require exactly
/// `READ_STATES`; writes (`Put`, `Delete`) require exactly `WRITE_STATES`.
/// Requiring both everywhere (the removed VM fast table did exactly that)
/// rejects legitimate read-only calls that hold only `READ_STATES`.
#[test]
fn storage_syscalls_require_per_operation_flags() {
    const STORAGE_FLAGS: &[(&str, CallFlags)] = &[
        ("System.Storage.GetContext", CallFlags::READ_STATES),
        ("System.Storage.GetReadOnlyContext", CallFlags::READ_STATES),
        ("System.Storage.AsReadOnly", CallFlags::READ_STATES),
        ("System.Storage.Get", CallFlags::READ_STATES),
        ("System.Storage.Find", CallFlags::READ_STATES),
        ("System.Storage.Put", CallFlags::WRITE_STATES),
        ("System.Storage.Delete", CallFlags::WRITE_STATES),
    ];

    const STORAGE_LOCAL_FLAGS: &[(&str, CallFlags)] = &[
        ("System.Storage.Local.Get", CallFlags::READ_STATES),
        ("System.Storage.Local.Find", CallFlags::READ_STATES),
        ("System.Storage.Local.Put", CallFlags::WRITE_STATES),
        ("System.Storage.Local.Delete", CallFlags::WRITE_STATES),
    ];

    for faun in [false, true] {
        let registry = default_interops(faun, false).expect("default registry");
        let service = &registry.service;

        for (name, expected) in STORAGE_FLAGS {
            let descriptor = descriptor(service, name, faun);
            assert_eq!(
                descriptor.required_call_flags, *expected,
                "{name} must require exactly {expected:?} (faun={faun})"
            );
        }

        // Faun-only `System.Storage.Local.*` mirrors the same split.
        for (name, expected) in STORAGE_LOCAL_FLAGS {
            if !faun {
                assert!(service.get_method(name.as_bytes()).is_none());
                continue;
            }
            let descriptor = descriptor(service, name, true);
            assert_eq!(
                descriptor.required_call_flags, *expected,
                "{name} must require exactly {expected:?}"
            );
        }
    }
}

/// Differential tie to the Coq refinement model
/// (`formal/coqlib/gas_price_refinement.v`): the base price registered for
/// every host service must equal the model's tier value.
#[test]
fn registered_gas_prices_match_coq_refinement_model() {
    const PRICES: &[(&str, i64)] = &[
        // System.Contract
        ("System.Contract.Call", 1 << 15),
        ("System.Contract.GetCallFlags", 1 << 10),
        ("System.Contract.CreateStandardAccount", 0),
        ("System.Contract.CreateMultisigAccount", 0),
        ("System.Contract.CallNative", 0),
        ("System.Contract.NativeOnPersist", 0),
        ("System.Contract.NativePostPersist", 0),
        // System.Runtime
        ("System.Runtime.Platform", 1 << 3),
        ("System.Runtime.GetTrigger", 1 << 3),
        ("System.Runtime.GetNetwork", 1 << 3),
        ("System.Runtime.GetAddressVersion", 1 << 3),
        ("System.Runtime.GetTime", 1 << 3),
        ("System.Runtime.GetScriptContainer", 1 << 3),
        ("System.Runtime.LoadScript", 1 << 15),
        ("System.Runtime.GetExecutingScriptHash", 1 << 4),
        ("System.Runtime.GetCallingScriptHash", 1 << 4),
        ("System.Runtime.GetEntryScriptHash", 1 << 4),
        ("System.Runtime.CheckWitness", 1 << 10),
        ("System.Runtime.GetInvocationCounter", 1 << 4),
        ("System.Runtime.GetRandom", 0),
        ("System.Runtime.GasLeft", 1 << 4),
        ("System.Runtime.Log", 1 << 15),
        ("System.Runtime.Notify", 1 << 15),
        ("System.Runtime.GetNotifications", 1 << 12),
        ("System.Runtime.BurnGas", 1 << 4),
        ("System.Runtime.CurrentSigners", 1 << 4),
        // System.Storage
        ("System.Storage.GetContext", 1 << 4),
        ("System.Storage.GetReadOnlyContext", 1 << 4),
        ("System.Storage.AsReadOnly", 1 << 4),
        ("System.Storage.Get", 1 << 15),
        ("System.Storage.Put", 1 << 15),
        ("System.Storage.Delete", 1 << 15),
        ("System.Storage.Find", 1 << 15),
        ("System.Storage.Local.Get", 1 << 15),
        ("System.Storage.Local.Put", 1 << 15),
        ("System.Storage.Local.Delete", 1 << 15),
        ("System.Storage.Local.Find", 1 << 15),
        // System.Crypto
        ("System.Crypto.CheckSig", 1 << 15),
        ("System.Crypto.CheckMultisig", 0),
        // System.Iterator
        ("System.Iterator.Next", 1 << 15),
        ("System.Iterator.Value", 1 << 4),
    ];

    let registry = default_interops(true, false).expect("Faun registry");
    let service = &registry.service;

    for (name, expected) in PRICES {
        let descriptor = descriptor(service, name, true);
        assert_eq!(descriptor.price, *expected, "gas price for {name}");
    }
    assert_eq!(
        service.len(),
        PRICES.len(),
        "the registered syscall surface must match the authoritative table"
    );
}

/// The `HF_Huyao` fixed prices (C# `[InteropPrice(Hardfork.HF_Huyao, ...)]`)
/// must be registered alongside the base prices.
#[test]
fn registered_huyao_prices_match_csharp() {
    const HUYAO_PRICES: &[(&str, i64)] = &[
        ("System.Contract.Call", 2_205_000),
        ("System.Contract.CallNative", 1_130_000),
        ("System.Contract.GetCallFlags", 5_933),
        ("System.Contract.CreateStandardAccount", 41_833),
        ("System.Contract.CreateMultisigAccount", 62_153),
        ("System.Runtime.Platform", 8_867),
        ("System.Runtime.GetNetwork", 9_833),
        ("System.Runtime.GetAddressVersion", 8_867),
        ("System.Runtime.GetTime", 9_033),
        ("System.Runtime.GetScriptContainer", 36_533),
        ("System.Runtime.LoadScript", 700_000),
        ("System.Runtime.CheckWitness", 1_035_000),
        ("System.Runtime.GetInvocationCounter", 10_267),
        ("System.Runtime.GetRandom", 15_833),
        ("System.Runtime.Log", 31_767),
        ("System.Runtime.Notify", 2_245_000),
        ("System.Runtime.GetNotifications", 205_000),
        ("System.Runtime.BurnGas", 3_067),
        ("System.Runtime.CurrentSigners", 18_429),
        ("System.Crypto.CheckSig", 2_326_967),
        ("System.Storage.GetContext", 12_200),
        ("System.Storage.GetReadOnlyContext", 11_933),
        ("System.Storage.Find", 6_799_366),
        ("System.Storage.Local.Find", 6_799_366),
        ("System.Storage.FindWithStart", 6_799_366),
        ("System.Storage.Local.FindWithStart", 6_799_366),
        ("System.Iterator.Next", 3_367),
        ("System.Iterator.Value", 19_173),
    ];

    let registry = default_interops(true, true).expect("Huyao registry");
    for (name, expected) in HUYAO_PRICES {
        let hash = neo_vm::interop_hash(name);
        let handler = registry
            .handlers
            .get(&hash)
            .unwrap_or_else(|| panic!("handler for {name}"));
        assert_eq!(
            handler.huyao_price,
            Some(*expected),
            "huyao price for {name}"
        );
    }
    // Services without a C# `InteropPrice` attribute keep their base price.
    for name in ["System.Storage.Get", "System.Runtime.GasLeft"] {
        let hash = neo_vm::interop_hash(name);
        let handler = registry
            .handlers
            .get(&hash)
            .unwrap_or_else(|| panic!("handler for {name}"));
        assert_eq!(
            handler.huyao_price, None,
            "unexpected huyao price for {name}"
        );
    }
}

/// C# v3.11.2 registers `FindWithStart` with `Hardfork.HF_Huyao`, so the
/// descriptors must only exist in Huyao-enabled registries. The two
/// descriptors are gated independently of `System.Storage.Local.*` (Faun),
/// mirroring the per-descriptor `Hardfork` arguments in C#.
#[test]
fn find_with_start_registered_only_since_huyao() {
    for (faun, huyao) in [(false, false), (true, false)] {
        let registry = default_interops(faun, huyao).expect("pre-Huyao registry");
        for name in [
            "System.Storage.FindWithStart",
            "System.Storage.Local.FindWithStart",
        ] {
            assert!(
                registry.service.get_method(name.as_bytes()).is_none(),
                "{name} must not be registered before HF_Huyao"
            );
        }
    }

    // Huyao-enabled registries carry both descriptors with the C# base
    // price (`1 << 15`), the `[InteropPrice]` coefficient and the flags.
    let registry = default_interops(true, true).expect("Huyao registry");
    for name in [
        "System.Storage.FindWithStart",
        "System.Storage.Local.FindWithStart",
    ] {
        let descriptor = descriptor(&registry.service, name, true);
        assert_eq!(descriptor.price, 1 << 15, "base price for {name}");
        assert_eq!(
            descriptor.required_call_flags,
            CallFlags::READ_STATES,
            "flags for {name}"
        );
        let hash = neo_vm::interop_hash(name);
        let handler = registry
            .handlers
            .get(&hash)
            .unwrap_or_else(|| panic!("handler for {name}"));
        assert_eq!(
            handler.huyao_price,
            Some(6_799_366),
            "huyao price for {name}"
        );
    }

    // Synthetic registry (Huyao without Faun): the Huyao descriptors are
    // still registered, independently of the Faun-only Local namespace.
    let registry = default_interops(false, true).expect("Huyao-only registry");
    assert!(
        registry
            .service
            .get_method(b"System.Storage.FindWithStart")
            .is_some()
    );
    assert!(
        registry
            .service
            .get_method(b"System.Storage.Local.FindWithStart")
            .is_some()
    );
    assert!(
        registry
            .service
            .get_method(b"System.Storage.Local.Find")
            .is_none()
    );
}
