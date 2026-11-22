use super::*;
use crate::application_engine::{
    EngineNativeServices, NativeRegistry, NativeStateReader, TEST_MODE_GAS, WitnessHashResolver,
};
use neo_primitives::{TriggerType, UInt160};
use neo_protocol::ProtocolSettings;
use neo_vm::{MAX_COMPOUND_NESTING_DEPTH, OpCode, StackItem, SyscallProvider};

struct NoHost;
impl SyscallProvider for NoHost {
    fn syscall(
        &mut self,
        api: u32,
        _ip: usize,
        _stack: &mut Vec<neo_vm::StackValue>,
    ) -> Result<(), String> {
        Err(format!("unsupported syscall 0x{api:08x}"))
    }
}

/// `levels` compounds one inside another, cycling through the three kinds.
fn nested(levels: usize) -> StackItem {
    let mut item = StackItem::from_array(Vec::<StackItem>::new());
    for level in 1..levels {
        item = match level % 3 {
            0 => StackItem::from_array(vec![item]),
            1 => StackItem::from_struct(vec![item]),
            _ => {
                let mut map = neo_vm::VmOrderedDictionary::with_capacity(1);
                map.insert(StackItem::from_i64(0), item);
                StackItem::from_map(map)
            }
        };
    }
    item
}

fn interpreter_refuses(item: StackItem) -> bool {
    let value = neo_vm::StackValue::try_from(item).expect("convertible value");
    match neo_vm::interpret_with_stack_and_syscalls(&[OpCode::RET as u8], vec![value], &mut NoHost)
    {
        Ok(_) => false,
        Err(message) => {
            assert_eq!(message, neo_vm::NESTING_DEPTH_ERROR);
            true
        }
    }
}

#[test]
fn nesting_check_counts_compound_levels() {
    assert!(!canonical_nesting_exceeds(&StackItem::from_i64(1), 0));
    assert!(!canonical_nesting_exceeds(&StackItem::Null, 0));
    assert!(canonical_nesting_exceeds(&nested(1), 0));
    assert!(!canonical_nesting_exceeds(&nested(1), 1));
    assert!(canonical_nesting_exceeds(&nested(4), 3));
    assert!(!canonical_nesting_exceeds(&nested(4), 4));
    // The deepest child decides, wherever it sits.
    let wide = StackItem::from_array(vec![nested(1), nested(3), StackItem::from_i64(1)]);
    assert!(canonical_nesting_exceeds(&wide, 3));
    assert!(!canonical_nesting_exceeds(&wide, 4));
}

#[test]
fn nesting_check_refuses_exactly_what_the_interpreter_refuses() {
    for levels in [1, 2, 63, 64, 65, 66, 100] {
        let exceeds = canonical_nesting_exceeds(&nested(levels), MAX_COMPOUND_NESTING_DEPTH);
        assert_eq!(
            exceeds,
            levels > MAX_COMPOUND_NESTING_DEPTH,
            "{levels} levels"
        );
        assert_eq!(
            exceeds,
            interpreter_refuses(nested(levels)),
            "{levels} levels"
        );
    }
}

#[test]
fn nesting_check_ends_on_a_value_that_contains_itself() {
    let outer = StackItem::from_array(Vec::<StackItem>::new());
    let inner = StackItem::from_array(vec![outer.clone()]);
    let StackItem::Array(array) = &outer else {
        unreachable!()
    };
    array.push(inner.clone()).expect("push");
    assert!(canonical_nesting_exceeds(
        &outer,
        MAX_COMPOUND_NESTING_DEPTH
    ));
    assert!(canonical_nesting_exceeds(
        &inner,
        MAX_COMPOUND_NESTING_DEPTH
    ));
    let holder = StackItem::from_struct(vec![StackItem::from_i64(1), inner]);
    assert!(canonical_nesting_exceeds(
        &holder,
        MAX_COMPOUND_NESTING_DEPTH
    ));
    // Break the cycle so the two arrays are freed.
    array.clear().expect("clear");
}

#[test]
fn nesting_check_stays_shallow_on_a_very_deep_value() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut item = StackItem::from_array(Vec::<StackItem>::new());
            for _ in 1..100_000 {
                item = StackItem::from_array(vec![item]);
            }
            assert!(canonical_nesting_exceeds(&item, MAX_COMPOUND_NESTING_DEPTH));
            // Dropping the chain would recurse once per level; take it
            // apart from the outside in instead.
            while let StackItem::Array(array) = &item {
                let Ok(child) = array.pop() else {
                    break;
                };
                item = child;
            }
        })
        .expect("spawn")
        .join()
        .expect("deep value must not overflow the native stack");
}

#[test]
fn interpreter_depth_guard_counts_executions_in_progress() {
    assert!(!external_vm_depth_exhausted());
    let mut guards: Vec<_> = (0..MAX_EXTERNAL_VM_DEPTH - 1)
        .map(|_| ExternalVmDepthGuard::enter())
        .collect();
    assert!(!external_vm_depth_exhausted());
    guards.push(ExternalVmDepthGuard::enter());
    assert!(external_vm_depth_exhausted());
    guards.pop();
    assert!(!external_vm_depth_exhausted());
    drop(guards);
    assert_eq!(EXTERNAL_VM_DEPTH.with(|depth| depth.get()), 0);
}

#[test]
fn static_slot_initialization_stays_on_canonical_engine() {
    assert!(script_uses_application_engine_host(&[
        OpCode::INITSSLOT as u8,
        1,
        OpCode::RET as u8,
    ]));
    assert!(!script_uses_application_engine_host(&[OpCode::RET as u8]));
}

#[test]
fn shared_caller_stack_never_truncates_initializer_arguments() {
    assert_eq!(external_result_stack_limit(0, true), None);
    assert_eq!(external_result_stack_limit(1, true), None);
    assert_eq!(external_result_stack_limit(-1, true), None);
}

#[test]
fn independent_stack_retains_declared_return_limit() {
    assert_eq!(external_result_stack_limit(0, false), Some(0));
    assert_eq!(external_result_stack_limit(1, false), Some(1));
    assert_eq!(external_result_stack_limit(-1, false), None);
}

/// The nesting bound the interpreter and the canonical engine share.
const BOUND: usize = MAX_COMPOUND_NESTING_DEPTH;
/// The deepest mixed value whose item count still fits the bound.
const BOUND_MIXED: usize = BOUND / 4 * 3;

struct NoopStateReader;
impl NativeStateReader for NoopStateReader {
    fn contract(
        &self,
        _snapshot: &PersistenceDataCache,
        _hash: &UInt160,
    ) -> crate::error::CoreResult<Option<ContractState>> {
        Ok(None)
    }
    fn current_index(&self, _snapshot: &PersistenceDataCache) -> crate::error::CoreResult<u32> {
        Ok(0)
    }
    fn is_blocked(
        &self,
        _snapshot: &PersistenceDataCache,
        _hash: &UInt160,
    ) -> crate::error::CoreResult<bool> {
        Ok(false)
    }
    fn whitelisted_fee(
        &self,
        _snapshot: &PersistenceDataCache,
        _hash: &UInt160,
        _method: &str,
        _param_count: u32,
    ) -> crate::error::CoreResult<Option<i64>> {
        Ok(None)
    }
    fn oracle_original_signers(
        &self,
        _snapshot: &PersistenceDataCache,
        _id: u64,
    ) -> crate::error::CoreResult<Option<Vec<neo_protocol::signer::Signer>>> {
        Ok(None)
    }
}

struct NoopResolver;
impl WitnessHashResolver for NoopResolver {
    fn resolve(
        &self,
        _verifiable: &dyn neo_primitives::Verifiable,
        _snapshot: &PersistenceDataCache,
    ) -> Vec<UInt160> {
        Vec::new()
    }
}

/// An engine running `script`, built the way the node builds one.
fn contract_engine(script: Vec<u8>) -> ApplicationEngine {
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(PersistenceDataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        EngineNativeServices {
            registry: NativeRegistry::empty(),
            state_reader: Arc::new(NoopStateReader),
            witness_hash_resolver: Arc::new(NoopResolver),
            policy_hash: UInt160::zero(),
            default_exec_fee_factor: 30,
            default_storage_price: 100_000,
        },
    )
    .expect("application engine");
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load script");
    engine
}

/// Runs `body` on a thread the size of the node's main thread. The ABI value
/// the bridge hands back derives one native frame set per nesting level, so
/// a value at the bound needs the room the node really gives it.
fn on_a_node_sized_thread(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(body)
        .expect("spawn node-sized thread")
        .join()
        .expect("the bridge fits the node's main thread stack");
}

/// One ABI value around both bridge conversions: in through
/// `replace_canonical_stack`, out through `export_canonical_stack`.
fn round_trip(value: VmStackValue) -> (Vec<StackItem>, Vec<VmStackValue>) {
    let script = vec![OpCode::RET as u8];
    let mut engine = contract_engine(script.clone());
    let execution = ExternalVmExecution {
        script,
        initial_stack: Vec::new(),
        initial_ip: 0,
        result_stack_limit: None,
        max_instructions: u64::MAX,
        instructions_executed: 0,
    };
    let mut host = ExternalVmHost::new(&mut engine, &execution);
    host.replace_canonical_stack(std::slice::from_ref(&value))
        .expect("the bridge takes the value in");
    let canonical = host
        .engine
        .vm_engine
        .engine()
        .current_context()
        .expect("context")
        .evaluation_stack()
        .to_vec();
    assert_eq!(canonical.len(), 1, "the bridge keeps one entry");
    let mut exported = Vec::new();
    host.export_canonical_stack(&mut exported)
        .expect("the bridge hands the value back");
    (canonical, exported)
}

/// An array nested `levels` levels around an empty array: one item a level,
/// which is the shape that reaches the bound itself.
fn nested_arrays(levels: usize) -> StackItem {
    let mut item = StackItem::from_array(Vec::<StackItem>::new());
    for _ in 1..levels {
        item = StackItem::from_array(vec![item]);
    }
    item
}

#[test]
fn bridge_round_trips_values_up_to_the_nesting_bound() {
    on_a_node_sized_thread(|| {
        for levels in [1, 2, 3, 64, 512, BOUND_MIXED] {
            let canonical = nested(levels);
            let external = VmStackValue::try_from(canonical.clone()).expect("initial stack value");
            let (replaced, exported) = round_trip(external.clone());
            assert_eq!(replaced, vec![canonical], "{levels} levels");
            assert_eq!(exported, vec![external], "{levels} levels");
        }
        // The bound itself, in the shape that costs one item a level.
        let canonical = nested_arrays(BOUND);
        let external = VmStackValue::try_from(canonical.clone()).expect("initial stack value");
        let (replaced, exported) = round_trip(external.clone());
        assert_eq!(replaced, vec![canonical]);
        assert_eq!(exported, vec![external]);
    });
}

/// The nesting depth of an ABI value, walked on an explicit stack.
fn external_depth(value: &VmStackValue) -> usize {
    let mut deepest = 0;
    let mut pending = vec![(value, 1)];
    while let Some((value, depth)) = pending.pop() {
        deepest = deepest.max(depth);
        match value {
            VmStackValue::Array(items) | VmStackValue::Struct(items) => {
                pending.extend(items.iter().map(|child| (child, depth + 1)));
            }
            VmStackValue::Map(entries) => {
                pending.extend(entries.iter().map(|(_, child)| (child, depth + 1)));
            }
            _ => {}
        }
    }
    deepest
}

/// Four executions are the most the depth guard allows in one call chain,
/// so four interpreter runs share this native stack; the innermost one
/// carries a value at the nesting bound through both conversions.
#[test]
fn four_bridge_layers_carry_a_value_at_the_bound() {
    on_a_node_sized_thread(|| {
        fn descend(remaining: usize) {
            if remaining == 0 {
                let value =
                    VmStackValue::try_from(nested_arrays(BOUND)).expect("initial stack value");
                let (replaced, exported) = round_trip(value.clone());
                assert_eq!(replaced.len(), 1);
                assert_eq!(external_depth(&exported[0]), BOUND);
                assert_eq!(exported, vec![value]);
                return;
            }
            let _layer = ExternalVmDepthGuard::enter();
            assert_eq!(
                external_vm_depth_exhausted(),
                remaining == 1,
                "{remaining} layers left"
            );
            descend(remaining - 1);
        }
        descend(MAX_EXTERNAL_VM_DEPTH);
        assert!(!external_vm_depth_exhausted());
    });
}
