use super::*;
use crate::Diagnostic;
use neo_error::CoreError;
use std::collections::HashMap;

/// Pins the vendored C# v3.10.1 `NativeContract.IsActive` predicate:
/// a descriptor is active when its `ActiveIn` hardfork is absent or active
/// and its `DeprecatedIn` hardfork is absent or not active.
#[test]
fn is_active_for_matches_v3101_and_form() {
    fn method(active: Option<Hardfork>, deprecated: Option<Hardfork>) -> NativeMethod {
        let mut m = NativeMethod::new("m", 0, true, 0, vec![], ContractParameterType::Void);
        if let Some(a) = active {
            m = m.with_active_in(a);
        }
        if let Some(d) = deprecated {
            m = m.with_deprecated_in(d);
        }
        m
    }
    // A hardfork is "active" iff it is in `passed`.
    fn checker(passed: Vec<Hardfork>) -> impl Fn(Hardfork, u32) -> bool {
        move |hf, _h| passed.contains(&hf)
    }
    let (c, g) = (Hardfork::HfCockatrice, Hardfork::HfGorgon);

    // Neither set -> always active.
    assert!(is_active_for(&method(None, None), checker(vec![]), 0));
    // Active-only.
    assert!(!is_active_for(&method(Some(g), None), checker(vec![]), 0));
    assert!(is_active_for(&method(Some(g), None), checker(vec![g]), 0));
    // Deprecated-only.
    assert!(is_active_for(&method(None, Some(g)), checker(vec![]), 0));
    assert!(!is_active_for(&method(None, Some(g)), checker(vec![g]), 0));
    // Both set: active(c) -> deprecated(g). It is inactive before c and
    // after g, active only in the window [c, g).
    assert!(!is_active_for(
        &method(Some(c), Some(g)),
        checker(vec![]),
        0
    ));
    assert!(is_active_for(
        &method(Some(c), Some(g)),
        checker(vec![c]),
        0
    ));
    assert!(!is_active_for(
        &method(Some(c), Some(g)),
        checker(vec![c, g]),
        0
    ));
    assert!(!is_active_for(
        &method(Some(c), Some(g)),
        checker(vec![g]),
        0
    ));
}

/// A minimal native contract exercising the event/parameter-name plumbing:
/// one ungated event at order 1, a dual registration at order 0 across
/// `HfEchidna` (V0 deprecated / V1 active), and one method with explicit
/// parameter names plus one without (the `arg{N}` fallback).
struct MockNative {
    methods: Vec<NativeMethod>,
    events: Vec<NativeEvent>,
}

impl MockNative {
    fn new() -> Self {
        Self {
            methods: vec![
                NativeMethod::new(
                    "named",
                    0,
                    true,
                    0,
                    vec![
                        ContractParameterType::Hash160,
                        ContractParameterType::Integer,
                    ],
                    ContractParameterType::Void,
                )
                .with_parameter_names(["account", "value"]),
                NativeMethod::new(
                    "unnamed",
                    0,
                    true,
                    0,
                    vec![ContractParameterType::String],
                    ContractParameterType::Void,
                ),
            ],
            events: vec![
                // Declared out of order on purpose: `events()` must sort by
                // the order index, not the declaration index.
                NativeEvent::new(1, "Ungated", &[("value", ContractParameterType::Integer)]),
                NativeEvent::new(0, "Dual", &[("a", ContractParameterType::Integer)])
                    .with_deprecated_in(Hardfork::HfEchidna),
                NativeEvent::new(
                    0,
                    "Dual",
                    &[
                        ("a", ContractParameterType::Integer),
                        ("b", ContractParameterType::Array),
                    ],
                )
                .with_active_in(Hardfork::HfEchidna),
            ],
        }
    }
}

impl<P> NativeContract<P> for MockNative
where
    P: NativeContractProvider + 'static,
{
    fn id(&self) -> i32 {
        -100
    }

    fn hash(&self) -> UInt160 {
        UInt160::zero()
    }

    fn name(&self) -> &str {
        "MockNative"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &self.events
    }

    fn invoke<D, B>(
        &self,
        _engine: &mut ApplicationEngine<P, D, B>,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        Err(CoreError::invalid_operation("MockNative is metadata-only"))
    }
}

fn settings_with_echidna_at(height: u32) -> ProtocolSettings {
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfEchidna, height);
    ProtocolSettings {
        hardforks,
        ..ProtocolSettings::mainnet()
    }
}

#[test]
fn events_filter_by_hardfork_and_sort_by_order() {
    let contract = MockNative::new();
    let settings = settings_with_echidna_at(100);

    // Pre-Echidna: the deprecated V0 `Dual` is active; ordering puts
    // order 0 before order 1 even though `Ungated` was declared first.
    let pre =
        <MockNative as NativeContract<NoNativeContractProvider>>::events(&contract, &settings, 0);
    assert_eq!(
        pre.iter()
            .map(|e| (e.name.as_str(), e.parameters.len()))
            .collect::<Vec<_>>(),
        vec![("Dual", 1), ("Ungated", 1)]
    );

    // Post-Echidna: V0 drops out, V1 (two parameters) replaces it.
    let post =
        <MockNative as NativeContract<NoNativeContractProvider>>::events(&contract, &settings, 100);
    assert_eq!(
        post.iter()
            .map(|e| (e.name.as_str(), e.parameters.len()))
            .collect::<Vec<_>>(),
        vec![("Dual", 2), ("Ungated", 1)]
    );

    // An unscheduled hardfork keeps the deprecated V0 active forever and
    // never activates V1 (C# IsActive semantics).
    let unscheduled = ProtocolSettings {
        hardforks: HashMap::new(),
        ..ProtocolSettings::mainnet()
    };
    let never = <MockNative as NativeContract<NoNativeContractProvider>>::events(
        &contract,
        &unscheduled,
        u32::MAX,
    );
    assert_eq!(
        never
            .iter()
            .map(|e| (e.name.as_str(), e.parameters.len()))
            .collect::<Vec<_>>(),
        vec![("Dual", 1), ("Ungated", 1)]
    );
}

#[test]
fn used_hardforks_include_event_attributes() {
    // C# `_usedHardforks` concatenates event ActiveIn/DeprecatedIn; the
    // mock's methods carry no hardforks, so Echidna can only come from the
    // events. This is what makes `is_initialize_block` refresh a manifest
    // at a boundary that only changes an event signature.
    let contract = MockNative::new();
    assert_eq!(
        <MockNative as NativeContract<NoNativeContractProvider>>::used_hardforks(&contract),
        vec![Hardfork::HfEchidna]
    );

    let settings = settings_with_echidna_at(100);
    let (initialize, hits) =
        <MockNative as NativeContract<NoNativeContractProvider>>::is_initialize_block(
            &contract, &settings, 100,
        );
    assert!(initialize);
    assert_eq!(hits, vec![Hardfork::HfEchidna]);
}

#[test]
fn manifest_composes_parameter_names_with_argn_fallback() {
    let contract = MockNative::new();
    let settings = settings_with_echidna_at(100);
    let state = build_native_contract_state_for::<NoNativeContractProvider, MockNative>(
        &contract, &settings, 0,
    );

    let named = state
        .manifest
        .abi
        .methods
        .iter()
        .find(|m| m.name == "named")
        .expect("named method");
    assert_eq!(
        named
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["account", "value"]
    );

    let unnamed = state
        .manifest
        .abi
        .methods
        .iter()
        .find(|m| m.name == "unnamed")
        .expect("unnamed method");
    assert_eq!(
        unnamed
            .parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        vec!["arg0"]
    );

    // The composed manifest carries the filtered, ordered event list.
    assert_eq!(
        state
            .manifest
            .abi
            .events
            .iter()
            .map(|e| (e.name.as_str(), e.parameters.len()))
            .collect::<Vec<_>>(),
        vec![("Dual", 1), ("Ungated", 1)]
    );
}

#[test]
fn native_method_constructor_accepts_borrowed_method_names() {
    let method = NativeMethod::new(
        "borrowedName",
        0,
        true,
        0,
        Vec::new(),
        ContractParameterType::Void,
    );

    assert_eq!(method.name, "borrowedName");
}
