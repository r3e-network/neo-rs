use neo_vm::{
    ExecutionEngine, ExecutionEngineLimits, InterpreterSemantics, OpCode, Script, StackValue,
    SyscallProvider, VmState, interpret_with_stack_and_syscalls,
};

struct SemanticsHost(InterpreterSemantics);

impl SyscallProvider for SemanticsHost {
    fn semantics(&self) -> InterpreterSemantics {
        self.0
    }

    fn syscall(
        &mut self,
        api: u32,
        _ip: usize,
        _stack: &mut Vec<StackValue>,
    ) -> Result<(), String> {
        Err(format!("unexpected syscall 0x{api:08x}"))
    }
}

fn semantics(pre_gorgon: bool) -> InterpreterSemantics {
    InterpreterSemantics {
        zero_shift_converts_to_integer: !pre_gorgon,
        pre_gorgon_collection_semantics: pre_gorgon,
    }
}

fn limits(pre_gorgon: bool) -> ExecutionEngineLimits {
    ExecutionEngineLimits {
        zero_shift_converts_to_integer: !pre_gorgon,
        pre_gorgon_collection_semantics: pre_gorgon,
        ..ExecutionEngineLimits::DEFAULT
    }
}

fn run_fast(script: &[u8], pre_gorgon: bool) -> (VmState, Vec<StackValue>) {
    let mut host = SemanticsHost(semantics(pre_gorgon));
    match interpret_with_stack_and_syscalls(script, Vec::new(), &mut host) {
        Ok(result) if result.state == VmState::Halt => (result.state, result.stack),
        Ok(_) => (VmState::Fault, Vec::new()),
        Err(_) => (VmState::Fault, Vec::new()),
    }
}

fn run_canonical(script: &[u8], pre_gorgon: bool) -> (VmState, Vec<StackValue>) {
    let mut engine = ExecutionEngine::new(None);
    engine.set_limits(limits(pre_gorgon));
    engine
        .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
        .expect("load differential script");
    let state = engine.execute();
    let stack = if state == VmState::Halt {
        engine
            .result_stack()
            .iter()
            .cloned()
            .map(StackValue::try_from)
            .collect::<Result<Vec<_>, _>>()
            .expect("canonical result converts to ABI values")
    } else {
        Vec::new()
    };
    (state, stack)
}

fn assert_engines_match(script: &[u8], pre_gorgon: bool) -> (VmState, Vec<StackValue>) {
    let fast = run_fast(script, pre_gorgon);
    let canonical = run_canonical(script, pre_gorgon);
    assert_eq!(
        fast, canonical,
        "pre_gorgon={pre_gorgon}, script={script:02x?}"
    );
    fast
}

fn push_i32(value: i32) -> Vec<u8> {
    let mut script = vec![OpCode::PUSHINT32.byte()];
    script.extend(value.to_le_bytes());
    script
}

#[test]
fn zero_shift_matches_canonical_and_preserves_pre_gorgon_types() {
    let inputs = [
        (
            vec![OpCode::PUSHDATA1.byte(), 1, 1],
            StackValue::ByteString(vec![1]),
            StackValue::Integer(1),
        ),
        (
            vec![OpCode::PUSH1.byte(), OpCode::NEWBUFFER.byte()],
            StackValue::ByteString(vec![0]),
            StackValue::Integer(0),
        ),
    ];

    for opcode in [OpCode::SHL, OpCode::SHR] {
        for (prefix, pre_gorgon_value, post_gorgon_value) in &inputs {
            let mut script = prefix.clone();
            script.extend([OpCode::PUSH0.byte(), opcode.byte(), OpCode::RET.byte()]);

            let (state, stack) = assert_engines_match(&script, true);
            assert_eq!(state, VmState::Halt);
            assert_eq!(stack, vec![pre_gorgon_value.clone()]);

            let (state, stack) = assert_engines_match(&script, false);
            assert_eq!(state, VmState::Halt);
            assert_eq!(stack, vec![post_gorgon_value.clone()]);
        }
    }
}

#[test]
fn haskey_max_item_size_boundary_matches_canonical_for_every_collection() {
    let collections = [
        vec![OpCode::PUSHDATA1.byte(), 1, 0xaa],
        vec![OpCode::PUSH1.byte(), OpCode::NEWBUFFER.byte()],
        vec![OpCode::PUSH1.byte(), OpCode::NEWARRAY.byte()],
        vec![OpCode::PUSH1.byte(), OpCode::NEWSTRUCT.byte()],
    ];

    for prefix in collections {
        let mut boundary = prefix.clone();
        boundary.extend(push_i32(neo_vm::MAX_ITEM_SIZE as i32));
        boundary.extend([OpCode::HASKEY.byte(), OpCode::RET.byte()]);

        let (state, stack) = assert_engines_match(&boundary, true);
        assert_eq!(state, VmState::Halt);
        assert_eq!(stack, vec![StackValue::Boolean(false)]);
        assert_eq!(assert_engines_match(&boundary, false).0, VmState::Fault);

        let mut in_bounds = prefix;
        in_bounds.extend([
            OpCode::PUSH0.byte(),
            OpCode::HASKEY.byte(),
            OpCode::RET.byte(),
        ]);
        for pre_gorgon in [true, false] {
            let (state, stack) = assert_engines_match(&in_bounds, pre_gorgon);
            assert_eq!(state, VmState::Halt);
            assert_eq!(stack, vec![StackValue::Boolean(true)]);
        }
    }
}
