use super::*;

impl ExternalVmHost<'_> {
    pub(super) fn invoke_canonical_syscall(
        &mut self,
        api: u32,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        self.replace_canonical_stack(stack)?;
        let target_depth = self.engine.vm_engine.engine().invocation_stack().len();
        let syscall_result = self.engine.vm_engine.engine_mut().on_syscall(api);
        if external_vm_trace_enabled() {
            eprintln!(
                "external syscall=0x{api:08x} target_depth={target_depth} depth={} state={:?} result={syscall_result:?}",
                self.engine.vm_engine.engine().invocation_stack().len(),
                self.engine.vm_engine.engine().state(),
            );
        }
        syscall_result.map_err(|error| error.to_string())?;
        let nested_result = self.execute_nested_to_depth(target_depth);
        if external_vm_trace_enabled() {
            eprintln!(
                "external nested target_depth={target_depth} depth={} state={:?} result={nested_result:?}",
                self.engine.vm_engine.engine().invocation_stack().len(),
                self.engine.vm_engine.engine().state(),
            );
        }
        nested_result?;
        self.export_canonical_stack(stack)
    }

    pub(super) fn invoke_canonical_callt(
        &mut self,
        token: u16,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        self.replace_canonical_stack(stack)?;
        let target_depth = self.engine.vm_engine.engine().invocation_stack().len();
        self.engine
            .vm_engine
            .engine_mut()
            .invoke_callt(token)
            .map_err(|error| error.to_string())?;
        self.execute_nested_to_depth(target_depth)?;
        self.export_canonical_stack(stack)
    }

    fn execute_nested_to_depth(&mut self, target_depth: usize) -> std::result::Result<(), String> {
        if self.engine.vm_engine.engine().invocation_stack().len() <= target_depth {
            return Ok(());
        }
        match self
            .engine
            .execute_until_invocation_stack_depth(target_depth)
        {
            VMState::FAULT => Err(self
                .engine
                .fault_exception()
                .unwrap_or("nested contract execution faulted")
                .to_string()),
            VMState::HALT
                if self.engine.vm_engine.engine().invocation_stack().len() > target_depth =>
            {
                Err("nested contract execution halted before returning to its caller".to_string())
            }
            _ => Ok(()),
        }
    }

    pub(super) fn replace_canonical_stack(
        &mut self,
        stack: &[VmStackValue],
    ) -> std::result::Result<(), String> {
        let items = stack
            .iter()
            .cloned()
            .map(|value| self.external_to_canonical(value))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let engine = self.engine.vm_engine.engine_mut();
        let context = engine
            .current_context_mut()
            .ok_or_else(|| "No current execution context for external VM bridge".to_string())?;
        let mut evaluation_stack = context.evaluation_stack_mut();
        evaluation_stack.clear();
        for item in items {
            evaluation_stack
                .push(item)
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) fn export_canonical_stack(
        &mut self,
        stack: &mut Vec<VmStackValue>,
    ) -> std::result::Result<(), String> {
        let items = {
            let engine = self.engine.vm_engine.engine();
            let context = engine
                .current_context()
                .ok_or_else(|| "No current execution context after host call".to_string())?;
            context.evaluation_stack().to_vec()
        };
        // Both conversions walk the value on an explicit stack, so this check
        // is here to end on a value that contains itself and to fault with the
        // interpreter's own message for one nested past its bound.
        if items
            .iter()
            .any(|item| canonical_nesting_exceeds(item, neo_vm::MAX_COMPOUND_NESTING_DEPTH))
        {
            return Err(neo_vm::NESTING_DEPTH_ERROR.to_string());
        }
        let exported = items
            .into_iter()
            .map(|item| self.canonical_to_external(item))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        *stack = exported;
        Ok(())
    }

    fn external_to_canonical(&self, value: VmStackValue) -> std::result::Result<StackItem, String> {
        // Children are built before the compound that holds them, on an
        // explicit work list: the interpreter admits values nested to its
        // limit, which no recursive conversion survives on a thread stack.
        enum Step {
            Enter(VmStackValue),
            Array(usize),
            Struct(usize),
            Map(usize),
        }

        let mut steps = vec![Step::Enter(value)];
        let mut built: Vec<StackItem> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Enter(value) => {
                    match value {
                        VmStackValue::Null => built.push(StackItem::Null),
                        VmStackValue::Boolean(value) => built.push(StackItem::from_bool(value)),
                        VmStackValue::Integer(value) => built.push(StackItem::from_i64(value)),
                        VmStackValue::BigInteger(bytes) => built.push(StackItem::from_int(
                            num_bigint::BigInt::from_signed_bytes_le(&bytes),
                        )),
                        VmStackValue::ByteString(bytes) => {
                            built.push(StackItem::from_byte_string(bytes));
                        }
                        VmStackValue::Buffer(bytes) => built.push(StackItem::from_buffer(bytes)),
                        VmStackValue::Array(items) => {
                            steps.push(Step::Array(items.len()));
                            steps.extend(items.into_iter().rev().map(Step::Enter));
                        }
                        VmStackValue::Struct(items) => {
                            steps.push(Step::Struct(items.len()));
                            steps.extend(items.into_iter().rev().map(Step::Enter));
                        }
                        VmStackValue::Map(entries) => {
                            steps.push(Step::Map(entries.len()));
                            let mut flat = Vec::with_capacity(entries.len().saturating_mul(2));
                            for (key, value) in entries {
                                flat.push(key);
                                flat.push(value);
                            }
                            steps.extend(flat.into_iter().rev().map(Step::Enter));
                        }
                        VmStackValue::Pointer(position) => {
                            let position = usize::try_from(position)
                                .map_err(|_| format!("negative external VM pointer: {position}"))?;
                            let script = self
                                .engine
                                .vm_engine
                                .engine()
                                .current_context()
                                .ok_or_else(|| "No script for external VM pointer".to_string())?
                                .script_arc();
                            built.push(StackItem::from_pointer(script, position));
                        }
                        VmStackValue::Interop(handle) | VmStackValue::Iterator(handle) => built
                            .push(self.runtime_items.get(&handle).cloned().ok_or_else(|| {
                                format!("unknown external VM host handle: {handle}")
                            })?),
                    }
                }
                Step::Array(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(StackItem::from_array(items));
                }
                Step::Struct(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(StackItem::from_struct(items));
                }
                Step::Map(len) => {
                    let mut flat = take_tail(&mut built, len.saturating_mul(2));
                    let mut map = neo_vm::VmOrderedDictionary::with_capacity(len);
                    let mut rest = flat.drain(..);
                    while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                        map.insert(key, value);
                    }
                    built.push(StackItem::from_map(map));
                }
            }
        }
        built
            .pop()
            .ok_or_else(|| "external VM value converted to nothing".to_string())
    }

    fn canonical_to_external(
        &mut self,
        item: StackItem,
    ) -> std::result::Result<VmStackValue, String> {
        // Same shape as `external_to_canonical`, in the other direction.
        enum Step {
            Enter(StackItem),
            Array(usize),
            Struct(usize),
            Map(usize),
        }

        let mut steps = vec![Step::Enter(item)];
        let mut built: Vec<VmStackValue> = Vec::new();
        while let Some(step) = steps.pop() {
            match step {
                Step::Enter(item) => match item {
                    StackItem::Null => built.push(VmStackValue::Null),
                    StackItem::Boolean(value) => built.push(VmStackValue::Boolean(value)),
                    StackItem::Integer(value) => built.push(match value.to_i64() {
                        Some(value) => VmStackValue::Integer(value),
                        None => VmStackValue::BigInteger(value.to_signed_bytes_le()),
                    }),
                    StackItem::ByteString(bytes) => built.push(VmStackValue::ByteString(bytes)),
                    StackItem::Buffer(buffer) => built.push(VmStackValue::Buffer(buffer.data())),
                    StackItem::Array(array) => {
                        steps.push(Step::Array(array.len()));
                        steps.extend(array.items().into_iter().rev().map(Step::Enter));
                    }
                    StackItem::Struct(structure) => {
                        steps.push(Step::Struct(structure.len()));
                        steps.extend(structure.items().into_iter().rev().map(Step::Enter));
                    }
                    StackItem::Map(map) => {
                        steps.push(Step::Map(map.len()));
                        let mut flat = Vec::with_capacity(map.len().saturating_mul(2));
                        for (key, value) in map.iter() {
                            flat.push(key);
                            flat.push(value);
                        }
                        steps.extend(flat.into_iter().rev().map(Step::Enter));
                    }
                    StackItem::Pointer(pointer) => built.push(VmStackValue::Pointer(
                        i64::try_from(pointer.position()).map_err(|_| {
                            "canonical pointer does not fit external VM i64".to_string()
                        })?,
                    )),
                    StackItem::InteropInterface(interface) => {
                        let handle = Arc::as_ptr(&interface).cast::<()>() as usize as u64;
                        self.runtime_items
                            .insert(handle, StackItem::InteropInterface(interface));
                        built.push(VmStackValue::Interop(handle));
                    }
                },
                Step::Array(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(VmStackValue::Array(items));
                }
                Step::Struct(len) => {
                    let items = take_tail(&mut built, len);
                    built.push(VmStackValue::Struct(items));
                }
                Step::Map(len) => {
                    let mut flat = take_tail(&mut built, len.saturating_mul(2));
                    let mut entries = Vec::with_capacity(len);
                    let mut rest = flat.drain(..);
                    while let (Some(key), Some(value)) = (rest.next(), rest.next()) {
                        entries.push((key, value));
                    }
                    built.push(VmStackValue::Map(entries));
                }
            }
        }
        built
            .pop()
            .ok_or_else(|| "canonical value converted to nothing".to_string())
    }
}
