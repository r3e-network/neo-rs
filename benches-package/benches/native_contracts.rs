//! Measures the per-transaction native-contract seeding cost.
//!
//! Block persistence used to deep-clone every native `ContractState` (NEF
//! scripts + manifests) once per transaction. The shared snapshot only bumps
//! `Arc` references; this benchmark keeps both paths visible so regressions are
//! caught.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::native::NativeRegistry;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::CallFlags;
use neo_vm::InteropService;
use std::sync::Arc;

fn make_engine() -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine")
}

fn bench_native_contract_seeding(c: &mut Criterion) {
    let engine = make_engine();
    let deep = engine.contracts_snapshot();
    let shared = engine.contracts_shared_snapshot();

    c.bench_function("native_contracts_deep_clone", |b| {
        b.iter(|| black_box(deep.clone()));
    });
    c.bench_function("native_contracts_arc_clone", |b| {
        b.iter(|| black_box(shared.clone()));
    });
}

fn bench_engine_initialization(c: &mut Criterion) {
    let base = make_engine();
    let contracts = base.contracts_snapshot();
    let cache = base.native_contract_cache_handle();
    let settings = ProtocolSettings::default();

    c.bench_function("application_engine_new", |b| {
        b.iter(|| {
            let engine = ApplicationEngine::new(
                TriggerType::Application,
                None,
                Arc::new(DataCache::new(false)),
                None,
                settings.clone(),
                TEST_MODE_GAS,
                None,
            )
            .expect("engine");
            black_box(engine);
        });
    });

    c.bench_function("application_engine_new_with_preloaded_native", |b| {
        b.iter(|| {
            let engine = ApplicationEngine::new_with_preloaded_native(
                TriggerType::Application,
                None,
                Arc::new(DataCache::new(false)),
                None,
                settings.clone(),
                TEST_MODE_GAS,
                contracts.clone(),
                Arc::clone(&cache),
                None,
            )
            .expect("engine");
            black_box(engine);
        });
    });

    c.bench_function("native_registry_new", |b| {
        b.iter(|| black_box(NativeRegistry::new()));
    });

    let names: Vec<String> = (0..50)
        .map(|index| format!("System.Perf.Test{index}"))
        .collect();
    c.bench_function("interop_service_register_50", |b| {
        b.iter(|| {
            let mut service = InteropService::new();
            for name in &names {
                service
                    .register_host_descriptor(name, 1, CallFlags::ALL)
                    .expect("register");
            }
            black_box(service);
        });
    });
}

criterion_group!(
    benches,
    bench_native_contract_seeding,
    bench_engine_initialization
);
criterion_main!(benches);
