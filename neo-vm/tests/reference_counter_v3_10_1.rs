//! The canonical engine counts stack references the way Neo.VM v3.10.1 does.
//!
//! [`RefModel`] is a transcript of the reference rules, written from
//! `ReferenceCounter.cs`, `EvaluationStack.cs`, `Slot.cs`,
//! `JumpTable.Compound.cs`, `JumpTable.Stack.cs`, `JumpTable.Slot.cs`,
//! `JumpTable.Control.cs` (`Ret`), `ExecutionEngine.cs` and `Types/Struct.cs`
//! at the tag. It is our reading of that source, not the reference
//! implementation itself; where the two disagree the source is right.
//!
//! * `AddStackReference(item, n)`: `Count += n`; a compound gets
//!   `StackReferences += n` and, when that made it `n`, every sub-item gets
//!   `AddStackReference(sub)`.
//! * `RemoveStackReference(item)`: anything but a compound takes one off
//!   `Count`; a compound only when `StackReferences != 0`, and then
//!   `StackReferences -= 1` and at zero every sub-item is released.
//! * The sub-items of a map are its keys followed by its values.
//! * Collections never call the counter. APPEND, SETITEM, REMOVE, CLEARITEMS
//!   and POPITEM do, and only when the collection is stack referenced, which
//!   includes being held by a referenced collection.
//! * Nothing is ever collected: a value that holds itself stays counted.
//! * After every instruction, a caught engine exception and the implicit RET
//!   included, `Count > MaxStackSize` faults.
//!
//! The model keeps the recursion of the reference, so it runs on a large
//! stack. The engine runs on a 1 MiB stack wherever a test builds a long
//! chain.

use neo_vm::stack_item::{Array, Map, Struct};
use neo_vm::{
    ExecutionEngine, ExecutionEngineLimits, Instruction, OpCode, ReferenceCounter, Script,
    StackItem, VmError, VmOrderedDictionary, VmResult, VmState,
};
use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const MAX_STACK_SIZE: usize = 2048;
const SMALL_STACK: usize = 1024 * 1024;
const LARGE_STACK: usize = 64 * 1024 * 1024;
const RUN_LIMIT: Duration = Duration::from_secs(120);
/// How long a script that stores a collection into itself may take.
const SELF_REFERENCE_LIMIT: Duration = Duration::from_secs(20);

/// Runs `body` on a thread with `stack_size` bytes of stack. A worker that has
/// not returned within `limit` fails the test and is left behind, not joined.
fn run_on_thread<T: Send + 'static>(
    label: &str,
    stack_size: usize,
    limit: Duration,
    body: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (sender, receiver) = mpsc::channel();
    let worker = thread::Builder::new()
        .stack_size(stack_size)
        .spawn(move || {
            let _ = sender.send(body());
        })
        .expect("spawn worker thread");
    match receiver.recv_timeout(limit) {
        Ok(value) => {
            worker.join().expect("worker thread exits");
            value
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("{label}: did not return within {limit:?}")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => match worker.join() {
            Err(payload) => std::panic::resume_unwind(payload),
            Ok(()) => panic!("{label}: worker ended without a result"),
        },
    }
}

fn on_small_stack<T: Send + 'static>(label: &str, body: impl FnOnce() -> T + Send + 'static) -> T {
    run_on_thread(label, SMALL_STACK, RUN_LIMIT, body)
}

fn on_large_stack<T: Send + 'static>(label: &str, body: impl FnOnce() -> T + Send + 'static) -> T {
    run_on_thread(label, LARGE_STACK, RUN_LIMIT, body)
}

// ---------------------------------------------------------------------------
// Script assembly
// ---------------------------------------------------------------------------

/// One VM instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Instr {
    /// An opcode without operand.
    Op(OpCode),
    /// A small non-negative integer: PUSH0..PUSH16 or PUSHINT32.
    Int(usize),
    /// `INITSLOT locals, arguments`.
    InitSlot(u8, u8),
    /// `INITSSLOT count`.
    InitSSlot(u8),
}

use Instr::{InitSSlot, InitSlot, Int, Op};

fn assemble(program: &[Instr]) -> Vec<u8> {
    let mut script = Vec::new();
    for instr in program {
        match *instr {
            Op(opcode) => script.push(opcode.byte()),
            Int(value) if value <= 16 => script.push(OpCode::PUSH0.byte() + value as u8),
            Int(value) => {
                script.push(OpCode::PUSHINT32.byte());
                script.extend(i32::try_from(value).expect("small operand").to_le_bytes());
            }
            InitSlot(locals, arguments) => {
                script.extend([OpCode::INITSLOT.byte(), locals, arguments]);
            }
            InitSSlot(count) => script.extend([OpCode::INITSSLOT.byte(), count]),
        }
    }
    script
}

fn instr_name(instr: Instr) -> String {
    match instr {
        Op(opcode) => format!("{opcode:?}"),
        Int(value) => format!("PUSH({value})"),
        InitSlot(locals, arguments) => format!("INITSLOT({locals},{arguments})"),
        InitSSlot(count) => format!("INITSSLOT({count})"),
    }
}

/// The instructions up to and including `index`, the last sixty at most.
fn listing(program: &[Instr], index: usize) -> String {
    let end = (index + 1).min(program.len());
    let start = end.saturating_sub(60);
    let names: Vec<String> = program[start..end].iter().map(|i| instr_name(*i)).collect();
    format!("#{start}..#{}: {}", end.saturating_sub(1), names.join(" "))
}

fn ops(opcodes: &[OpCode]) -> Vec<Instr> {
    opcodes.iter().map(|opcode| Op(*opcode)).collect()
}

fn repeat(prefix: &[Instr], round: &[Instr], rounds: usize) -> Vec<Instr> {
    let mut program = prefix.to_vec();
    for _ in 0..rounds {
        program.extend_from_slice(round);
    }
    program
}

fn concat(parts: &[&[Instr]]) -> Vec<Instr> {
    parts.iter().flat_map(|part| part.iter().copied()).collect()
}

/// `NEWARRAY0 DUP (NEWARRAY0 TUCK APPEND)×k`: A0 ⊃ A1 ⊃ … ⊃ Ak with A0 and Ak
/// on the stack. Every array but A0 and Ak is held by its parent only.
fn append_chain(k: usize) -> Vec<Instr> {
    use OpCode::*;
    repeat(&ops(&[NEWARRAY0, DUP]), &ops(&[NEWARRAY0, TUCK, APPEND]), k)
}

/// `PUSH1 NEWARRAY DUP (PUSH1 NEWARRAY TUCK PUSH0 SWAP SETITEM)×k`: every
/// round stores a new one-slot array into slot 0 of the previous one.
fn setitem_array_chain(k: usize) -> Vec<Instr> {
    use OpCode::*;
    repeat(
        &[Int(1), Op(NEWARRAY), Op(DUP)],
        &[
            Int(1),
            Op(NEWARRAY),
            Op(TUCK),
            Int(0),
            Op(SWAP),
            Op(SETITEM),
        ],
        k,
    )
}

/// `NEWMAP DUP (NEWMAP TUCK PUSH0 SWAP SETITEM)×k`: every round stores a new
/// map under key 0 of the previous one.
fn setitem_map_chain(k: usize) -> Vec<Instr> {
    use OpCode::*;
    repeat(
        &ops(&[NEWMAP, DUP]),
        &[Op(NEWMAP), Op(TUCK), Int(0), Op(SWAP), Op(SETITEM)],
        k,
    )
}

/// `NEWSTRUCT0 (NEWSTRUCT0 TUCK SWAP APPEND)×k`: a struct nested k deep, built
/// inside out because APPEND clones a struct value.
fn struct_chain(k: usize) -> Vec<Instr> {
    use OpCode::*;
    repeat(
        &ops(&[NEWSTRUCT0]),
        &ops(&[NEWSTRUCT0, TUCK, SWAP, APPEND]),
        k,
    )
}

/// An array of `n` items appended to an array that only its parent holds:
/// `NEWARRAY0 PUSH1 PACK` packs A into P, `DUP PUSH0 PICKITEM` fetches A.
fn append_into_nested(n: usize) -> Vec<Instr> {
    use OpCode::*;
    let mut program = ops(&[NEWARRAY0, PUSH1, PACK, DUP, PUSH0, PICKITEM]);
    program.extend([Int(n), Op(NEWARRAY), Op(APPEND)]);
    program
}

/// `PUSH300 NEWARRAY PUSH1 PACK DUP <release>`: a 300-item array packed into
/// P and then taken out of it again.
fn packed_child_release(release: &[Instr]) -> Vec<Instr> {
    use OpCode::*;
    let mut program = vec![Int(300), Op(NEWARRAY), Int(1), Op(PACK), Op(DUP)];
    program.extend_from_slice(release);
    program
}

/// `(NEWARRAY0 DUP DUP APPEND DROP)×rounds`: every round leaves behind one
/// array that holds itself and that nothing else refers to.
fn abandoned_cycles(rounds: usize) -> Vec<Instr> {
    use OpCode::*;
    repeat(&[], &ops(&[NEWARRAY0, DUP, DUP, APPEND, DROP]), rounds)
}

// ---------------------------------------------------------------------------
// Reference model of the Neo.VM v3.10.1 counter
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RItem {
    Null,
    Int(i64),
    Compound(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Array,
    Struct,
    Map,
}

struct RCompound {
    kind: Kind,
    /// Array and Struct: the items. Map: key, value, key, value, … in
    /// insertion order.
    items: Vec<RItem>,
    stack_references: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlotKind {
    Static,
    Local,
    Argument,
}

const LOAD_STATIC: [OpCode; 3] = [OpCode::LDSFLD0, OpCode::LDSFLD1, OpCode::LDSFLD2];
const STORE_STATIC: [OpCode; 3] = [OpCode::STSFLD0, OpCode::STSFLD1, OpCode::STSFLD2];
const LOAD_LOCAL: [OpCode; 3] = [OpCode::LDLOC0, OpCode::LDLOC1, OpCode::LDLOC2];
const STORE_LOCAL: [OpCode; 3] = [OpCode::STLOC0, OpCode::STLOC1, OpCode::STLOC2];
const LOAD_ARGUMENT: [OpCode; 3] = [OpCode::LDARG0, OpCode::LDARG1, OpCode::LDARG2];
const STORE_ARGUMENT: [OpCode; 3] = [OpCode::STARG0, OpCode::STARG1, OpCode::STARG2];

/// `(slot, index, is a store)` for the slot opcodes the tests use.
fn slot_access(opcode: OpCode) -> Option<(SlotKind, usize, bool)> {
    let tables = [
        (SlotKind::Static, false, LOAD_STATIC),
        (SlotKind::Static, true, STORE_STATIC),
        (SlotKind::Local, false, LOAD_LOCAL),
        (SlotKind::Local, true, STORE_LOCAL),
        (SlotKind::Argument, false, LOAD_ARGUMENT),
        (SlotKind::Argument, true, STORE_ARGUMENT),
    ];
    tables.into_iter().find_map(|(kind, store, table)| {
        table
            .iter()
            .position(|candidate| *candidate == opcode)
            .map(|index| (kind, index, store))
    })
}

/// Why the model stopped.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ModelFault {
    /// The instruction itself failed.
    Instruction(String),
    /// `PostExecuteInstruction` found `Count > MaxStackSize`.
    Limit(String),
}

#[derive(Default)]
struct RefModel {
    count: usize,
    heap: Vec<RCompound>,
    stack: Vec<RItem>,
    static_fields: Option<Vec<RItem>>,
    local_variables: Option<Vec<RItem>>,
    arguments: Option<Vec<RItem>>,
}

impl RefModel {
    fn alloc(&mut self, kind: Kind, items: Vec<RItem>) -> usize {
        self.heap.push(RCompound {
            kind,
            items,
            stack_references: 0,
        });
        self.heap.len() - 1
    }

    /// `CompoundType.SubItems`.
    fn sub_items(&self, id: usize) -> Vec<RItem> {
        let compound = &self.heap[id];
        match compound.kind {
            Kind::Map => {
                let keys = compound.items.iter().step_by(2);
                let values = compound.items.iter().skip(1).step_by(2);
                keys.chain(values).copied().collect()
            }
            Kind::Array | Kind::Struct => compound.items.clone(),
        }
    }

    /// `CompoundType.Count`.
    fn len_of(&self, id: usize) -> usize {
        let compound = &self.heap[id];
        match compound.kind {
            Kind::Map => compound.items.len() / 2,
            Kind::Array | Kind::Struct => compound.items.len(),
        }
    }

    /// `ReferenceCounter.AddStackReference`.
    fn add_stack_reference(&mut self, item: RItem, n: usize) {
        self.count += n;
        if let RItem::Compound(id) = item {
            self.heap[id].stack_references += n;
            if self.heap[id].stack_references == n {
                for sub in self.sub_items(id) {
                    self.add_stack_reference(sub, 1);
                }
            }
        }
    }

    /// `ReferenceCounter.RemoveStackReference`.
    fn remove_stack_reference(&mut self, item: RItem) {
        match item {
            RItem::Compound(id) => {
                if self.heap[id].stack_references != 0 {
                    self.count -= 1;
                    self.heap[id].stack_references -= 1;
                    if self.heap[id].stack_references == 0 {
                        for sub in self.sub_items(id) {
                            self.remove_stack_reference(sub);
                        }
                    }
                }
            }
            RItem::Null | RItem::Int(_) => self.count -= 1,
        }
    }

    fn is_stack_referenced(&self, id: usize) -> bool {
        self.heap[id].stack_references != 0
    }

    fn push(&mut self, item: RItem) {
        self.stack.push(item);
        self.add_stack_reference(item, 1);
    }

    fn pop(&mut self) -> Result<RItem, String> {
        let item = self.stack.pop().ok_or("stack underflow")?;
        self.remove_stack_reference(item);
        Ok(item)
    }

    fn peek(&self, from_top: usize) -> Result<RItem, String> {
        let len = self.stack.len();
        if from_top >= len {
            return Err("peek out of range".into());
        }
        Ok(self.stack[len - 1 - from_top])
    }

    /// `EvaluationStack.Insert(index, item)`, the index counted from the top.
    fn insert(&mut self, from_top: usize, item: RItem) -> Result<(), String> {
        let len = self.stack.len();
        if from_top > len {
            return Err("insert out of range".into());
        }
        self.stack.insert(len - from_top, item);
        self.add_stack_reference(item, 1);
        Ok(())
    }

    /// `EvaluationStack.Remove(index)`, the index counted from the top.
    fn remove(&mut self, from_top: usize) -> Result<RItem, String> {
        let len = self.stack.len();
        if from_top >= len {
            return Err("remove out of range".into());
        }
        let item = self.stack.remove(len - 1 - from_top);
        self.remove_stack_reference(item);
        Ok(item)
    }

    /// `EvaluationStack.Swap`: no counter call.
    fn swap(&mut self, a_from_top: usize, b_from_top: usize) -> Result<(), String> {
        let len = self.stack.len();
        if a_from_top >= len || b_from_top >= len {
            return Err("swap out of range".into());
        }
        self.stack.swap(len - 1 - a_from_top, len - 1 - b_from_top);
        Ok(())
    }

    /// `Pop().GetInteger()`.
    fn pop_integer(&mut self) -> Result<i64, String> {
        match self.pop()? {
            RItem::Int(value) => Ok(value),
            RItem::Null | RItem::Compound(_) => Err("not an integer".into()),
        }
    }

    /// `Pop<PrimitiveType>()`; integers are the only primitives the model has.
    fn pop_primitive(&mut self) -> Result<i64, String> {
        self.pop_integer()
    }

    fn pop_compound(&mut self) -> Result<usize, String> {
        match self.pop()? {
            RItem::Compound(id) => Ok(id),
            RItem::Null | RItem::Int(_) => Err("not a compound".into()),
        }
    }

    /// `Pop<VMArray>()`: an Array or a Struct.
    fn pop_array(&mut self) -> Result<usize, String> {
        let id = self.pop_compound()?;
        if self.heap[id].kind == Kind::Map {
            return Err("not an array".into());
        }
        Ok(id)
    }

    fn array_index(&self, id: usize, index: i64) -> Result<usize, String> {
        usize::try_from(index)
            .ok()
            .filter(|index| *index < self.heap[id].items.len())
            .ok_or_else(|| "index out of range".to_string())
    }

    /// Position of `key` in the flat item list of map `id`.
    fn map_position(&self, id: usize, key: i64) -> Option<usize> {
        self.heap[id]
            .items
            .chunks(2)
            .position(|entry| entry[0] == RItem::Int(key))
            .map(|entry| entry * 2)
    }

    /// `Struct.Clone(limits)`: breadth first, one unit of a
    /// `MaxStackSize - 1` budget per sub-item, nested structs cloned and
    /// everything else shared.
    fn clone_struct(&mut self, id: usize) -> Result<usize, String> {
        let mut budget = MAX_STACK_SIZE as i64 - 1;
        let result = self.alloc(Kind::Struct, Vec::new());
        let mut queue = VecDeque::from([(result, id)]);
        while let Some((target, source)) = queue.pop_front() {
            for item in self.heap[source].items.clone() {
                budget -= 1;
                if budget < 0 {
                    return Err("Beyond struct subitem clone limits!".into());
                }
                match item {
                    RItem::Compound(nested) if self.heap[nested].kind == Kind::Struct => {
                        let copy = self.alloc(Kind::Struct, Vec::new());
                        self.heap[target].items.push(RItem::Compound(copy));
                        queue.push_back((copy, nested));
                    }
                    other => self.heap[target].items.push(other),
                }
            }
        }
        Ok(result)
    }

    fn clone_if_struct(&mut self, item: RItem) -> Result<RItem, String> {
        match item {
            RItem::Compound(id) if self.heap[id].kind == Kind::Struct => {
                Ok(RItem::Compound(self.clone_struct(id)?))
            }
            other => Ok(other),
        }
    }

    fn slot(&mut self, kind: SlotKind) -> &mut Option<Vec<RItem>> {
        match kind {
            SlotKind::Static => &mut self.static_fields,
            SlotKind::Local => &mut self.local_variables,
            SlotKind::Argument => &mut self.arguments,
        }
    }

    /// `new Slot(count, referenceCounter)`.
    fn new_slot(&mut self, count: usize) -> Vec<RItem> {
        self.add_stack_reference(RItem::Null, count);
        vec![RItem::Null; count]
    }

    /// `Slot.ClearReferences()`.
    fn clear_references(&mut self, kind: SlotKind) {
        if let Some(items) = self.slot(kind).clone() {
            for item in items {
                self.remove_stack_reference(item);
            }
        }
    }

    /// One instruction followed by `PostExecuteInstruction`.
    fn step(&mut self, instr: Instr) -> Result<(), ModelFault> {
        self.execute(instr).map_err(ModelFault::Instruction)?;
        self.post_execute_instruction()
    }

    /// The RET that `ExecuteNext` substitutes at the end of the script: the
    /// evaluation stack becomes the result stack without a counter call,
    /// `ContextUnloaded` releases the slots, and the limit is checked.
    fn implicit_ret(&mut self) -> Result<(), ModelFault> {
        self.clear_references(SlotKind::Static);
        self.clear_references(SlotKind::Local);
        self.clear_references(SlotKind::Argument);
        self.post_execute_instruction()
    }

    fn post_execute_instruction(&self) -> Result<(), ModelFault> {
        if self.count > MAX_STACK_SIZE {
            return Err(ModelFault::Limit(format!(
                "MaxStackSize exceed: {}/{MAX_STACK_SIZE}",
                self.count
            )));
        }
        Ok(())
    }

    fn execute(&mut self, instr: Instr) -> Result<(), String> {
        match instr {
            Int(value) => self.push(RItem::Int(value as i64)),
            InitSSlot(count) => {
                if self.static_fields.is_some() {
                    return Err("INITSSLOT cannot be executed twice".into());
                }
                if count == 0 {
                    return Err("INITSSLOT operand cannot be 0".into());
                }
                let slot = self.new_slot(usize::from(count));
                self.static_fields = Some(slot);
            }
            InitSlot(locals, arguments) => {
                if self.local_variables.is_some() || self.arguments.is_some() {
                    return Err("INITSLOT cannot be executed twice".into());
                }
                if locals == 0 && arguments == 0 {
                    return Err("INITSLOT operands cannot both be 0".into());
                }
                if locals > 0 {
                    let slot = self.new_slot(usize::from(locals));
                    self.local_variables = Some(slot);
                }
                if arguments > 0 {
                    let mut items = Vec::new();
                    for _ in 0..arguments {
                        items.push(self.pop()?);
                    }
                    // `new Slot(items, referenceCounter)`.
                    for item in &items {
                        self.add_stack_reference(*item, 1);
                    }
                    self.arguments = Some(items);
                }
            }
            Op(opcode) => self.execute_opcode(opcode)?,
        }
        Ok(())
    }

    fn execute_opcode(&mut self, opcode: OpCode) -> Result<(), String> {
        use OpCode::*;

        let byte = opcode.byte();
        if (PUSH0.byte()..=PUSH16.byte()).contains(&byte) {
            self.push(RItem::Int(i64::from(byte - PUSH0.byte())));
            return Ok(());
        }

        if let Some((kind, index, store)) = slot_access(opcode) {
            let len = self
                .slot(kind)
                .as_ref()
                .map(Vec::len)
                .ok_or("slot has not been initialized")?;
            if index >= len {
                return Err("slot index out of range".into());
            }
            if store {
                // `slot[index] = engine.Pop()`: release the old value, store,
                // reference the new one.
                let value = self.pop()?;
                let old = self.slot(kind).as_ref().expect("slot exists")[index];
                self.remove_stack_reference(old);
                self.slot(kind).as_mut().expect("slot exists")[index] = value;
                self.add_stack_reference(value, 1);
            } else {
                let value = self.slot(kind).as_ref().expect("slot exists")[index];
                self.push(value);
            }
            return Ok(());
        }

        match opcode {
            PUSHNULL => self.push(RItem::Null),
            NEWARRAY0 | NEWSTRUCT0 | NEWMAP => {
                let kind = match opcode {
                    NEWARRAY0 => Kind::Array,
                    NEWSTRUCT0 => Kind::Struct,
                    _ => Kind::Map,
                };
                let id = self.alloc(kind, Vec::new());
                self.push(RItem::Compound(id));
            }
            NEWARRAY | NEWSTRUCT => {
                let n = self.pop_integer()?;
                if n < 0 || n > MAX_STACK_SIZE as i64 {
                    return Err("collection size out of range".into());
                }
                let kind = if opcode == NEWARRAY {
                    Kind::Array
                } else {
                    Kind::Struct
                };
                let id = self.alloc(kind, vec![RItem::Null; n as usize]);
                self.push(RItem::Compound(id));
            }
            DUP => {
                let top = self.peek(0)?;
                self.push(top);
            }
            OVER => {
                let second = self.peek(1)?;
                self.push(second);
            }
            PICK => {
                let n = self.pop_integer()?;
                let n = usize::try_from(n).map_err(|_| "negative PICK index")?;
                let item = self.peek(n)?;
                self.push(item);
            }
            TUCK => {
                let top = self.peek(0)?;
                self.insert(2, top)?;
            }
            SWAP => self.swap(0, 1)?,
            ROT => {
                self.swap(1, 2)?;
                self.swap(0, 1)?;
            }
            DROP => {
                self.pop()?;
            }
            NIP => {
                self.remove(1)?;
            }
            CLEAR => {
                // `EvaluationStack.Clear()` releases every item.
                for item in std::mem::take(&mut self.stack) {
                    self.remove_stack_reference(item);
                }
            }
            PACK | PACKSTRUCT => {
                let n = self.pop_integer()?;
                if n < 0 || n as usize > self.stack.len() {
                    return Err("PACK size out of range".into());
                }
                let kind = if opcode == PACK {
                    Kind::Array
                } else {
                    Kind::Struct
                };
                let id = self.alloc(kind, Vec::new());
                for _ in 0..n {
                    let item = self.pop()?;
                    self.heap[id].items.push(item);
                }
                self.push(RItem::Compound(id));
            }
            PACKMAP => {
                let n = self.pop_integer()?;
                if n < 0 || n as usize * 2 > self.stack.len() {
                    return Err("PACKMAP size out of range".into());
                }
                let id = self.alloc(Kind::Map, Vec::new());
                for _ in 0..n {
                    let key = self.pop_primitive()?;
                    let value = self.pop()?;
                    match self.map_position(id, key) {
                        Some(position) => self.heap[id].items[position + 1] = value,
                        None => self.heap[id].items.extend([RItem::Int(key), value]),
                    }
                }
                self.push(RItem::Compound(id));
            }
            UNPACK => {
                let id = self.pop_compound()?;
                let items = self.heap[id].items.clone();
                match self.heap[id].kind {
                    Kind::Map => {
                        for entry in items.chunks(2).rev() {
                            self.push(entry[1]);
                            self.push(entry[0]);
                        }
                    }
                    Kind::Array | Kind::Struct => {
                        for item in items.iter().rev() {
                            self.push(*item);
                        }
                    }
                }
                let count = self.len_of(id);
                self.push(RItem::Int(count as i64));
            }
            SIZE => {
                let id = self.pop_compound()?;
                let count = self.len_of(id);
                self.push(RItem::Int(count as i64));
            }
            KEYS => {
                let id = self.pop_compound()?;
                if self.heap[id].kind != Kind::Map {
                    return Err("KEYS needs a map".into());
                }
                let keys = self.heap[id].items.iter().step_by(2).copied().collect();
                let array = self.alloc(Kind::Array, keys);
                self.push(RItem::Compound(array));
            }
            VALUES => {
                let id = self.pop_compound()?;
                let values: Vec<RItem> = match self.heap[id].kind {
                    Kind::Map => self.heap[id]
                        .items
                        .iter()
                        .skip(1)
                        .step_by(2)
                        .copied()
                        .collect(),
                    Kind::Array | Kind::Struct => self.heap[id].items.clone(),
                };
                let array = self.alloc(Kind::Array, Vec::new());
                for value in values {
                    let value = self.clone_if_struct(value)?;
                    self.heap[array].items.push(value);
                }
                self.push(RItem::Compound(array));
            }
            REVERSEITEMS => {
                let id = self.pop_array()?;
                self.heap[id].items.reverse();
            }
            PICKITEM => {
                let key = self.pop_primitive()?;
                let id = self.pop_compound()?;
                let item = match self.heap[id].kind {
                    Kind::Map => {
                        let position = self.map_position(id, key).ok_or("key not found")?;
                        self.heap[id].items[position + 1]
                    }
                    Kind::Array | Kind::Struct => {
                        let index = self.array_index(id, key)?;
                        self.heap[id].items[index]
                    }
                };
                self.push(item);
            }
            APPEND => {
                let item = self.pop()?;
                let id = self.pop_array()?;
                let item = self.clone_if_struct(item)?;
                self.heap[id].items.push(item);
                if self.is_stack_referenced(id) {
                    self.add_stack_reference(item, 1);
                }
            }
            SETITEM => {
                let value = self.pop()?;
                let value = self.clone_if_struct(value)?;
                let key = self.pop_primitive()?;
                let id = self.pop_compound()?;
                match self.heap[id].kind {
                    Kind::Map => {
                        let existing = self.map_position(id, key);
                        if self.is_stack_referenced(id) {
                            match existing {
                                None => self.add_stack_reference(RItem::Int(key), 1),
                                Some(position) => {
                                    let old = self.heap[id].items[position + 1];
                                    self.remove_stack_reference(old);
                                }
                            }
                            self.add_stack_reference(value, 1);
                        }
                        match existing {
                            None => self.heap[id].items.extend([RItem::Int(key), value]),
                            Some(position) => self.heap[id].items[position + 1] = value,
                        }
                    }
                    Kind::Array | Kind::Struct => {
                        let index = self.array_index(id, key)?;
                        if self.is_stack_referenced(id) {
                            let old = self.heap[id].items[index];
                            self.remove_stack_reference(old);
                        }
                        self.heap[id].items[index] = value;
                        if self.is_stack_referenced(id) {
                            self.add_stack_reference(value, 1);
                        }
                    }
                }
            }
            REMOVE => {
                let key = self.pop_primitive()?;
                let id = self.pop_compound()?;
                match self.heap[id].kind {
                    Kind::Map => {
                        if let Some(position) = self.map_position(id, key) {
                            let old = self.heap[id].items[position + 1];
                            self.heap[id].items.drain(position..position + 2);
                            if self.is_stack_referenced(id) {
                                self.remove_stack_reference(RItem::Int(key));
                                self.remove_stack_reference(old);
                            }
                        }
                    }
                    Kind::Array | Kind::Struct => {
                        let index = self.array_index(id, key)?;
                        let item = self.heap[id].items.remove(index);
                        if self.is_stack_referenced(id) {
                            self.remove_stack_reference(item);
                        }
                    }
                }
            }
            CLEARITEMS => {
                let id = self.pop_compound()?;
                let sub_items = self.sub_items(id);
                self.heap[id].items.clear();
                if self.is_stack_referenced(id) {
                    for sub in sub_items {
                        self.remove_stack_reference(sub);
                    }
                }
            }
            POPITEM => {
                let id = self.pop_array()?;
                let item = *self.heap[id]
                    .items
                    .last()
                    .ok_or("POPITEM on an empty array")?;
                self.push(item);
                self.heap[id].items.pop();
                if self.is_stack_referenced(id) {
                    self.remove_stack_reference(item);
                }
            }
            other => return Err(format!("the reference model does not cover {other:?}")),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Fingerprints: a value graph written out without recursion
// ---------------------------------------------------------------------------

/// What a fingerprint needs to know about one item.
enum Shape<I> {
    Null,
    Integer(String),
    Other(String),
    Compound {
        kind: char,
        identity: (u8, usize),
        stack_references: usize,
        children: Vec<I>,
    },
}

/// Writes out `roots` and everything reachable from them: values, the order
/// of sub-items, which compounds are the same object, and the stack
/// reference count of every compound. Compounds are numbered in the order
/// they are first met, so two graphs of the same shape give the same text.
fn fingerprint<I: Clone>(roots: &[I], shape: impl Fn(&I) -> Shape<I>) -> String {
    enum Work<I> {
        Item(I),
        Close,
    }

    let mut text = String::new();
    let mut numbers: HashMap<(u8, usize), usize> = HashMap::new();
    let mut work: Vec<Work<I>> = roots.iter().rev().cloned().map(Work::Item).collect();
    while let Some(next) = work.pop() {
        let item = match next {
            Work::Close => {
                text.push_str("] ");
                continue;
            }
            Work::Item(item) => item,
        };
        match shape(&item) {
            Shape::Null => text.push_str("null "),
            Shape::Integer(value) => {
                let _ = write!(text, "{value} ");
            }
            Shape::Other(description) => {
                let _ = write!(text, "<{description}> ");
            }
            Shape::Compound {
                kind,
                identity,
                stack_references,
                children,
            } => {
                if let Some(number) = numbers.get(&identity) {
                    let _ = write!(text, "@{number} ");
                    continue;
                }
                let number = numbers.len();
                numbers.insert(identity, number);
                let _ = write!(text, "{kind}{number}x{stack_references}[ ");
                work.push(Work::Close);
                work.extend(children.into_iter().rev().map(Work::Item));
            }
        }
    }
    text
}

fn model_fingerprint(model: &RefModel) -> String {
    fingerprint(&model.stack, |item| match *item {
        RItem::Null => Shape::Null,
        RItem::Int(value) => Shape::Integer(value.to_string()),
        RItem::Compound(id) => {
            let compound = &model.heap[id];
            Shape::Compound {
                kind: match compound.kind {
                    Kind::Array => 'A',
                    Kind::Struct => 'S',
                    Kind::Map => 'M',
                },
                identity: (0, id),
                stack_references: compound.stack_references,
                children: compound.items.clone(),
            }
        }
    })
}

fn engine_fingerprint(roots: &[StackItem]) -> String {
    fingerprint(roots, |item| match item {
        StackItem::Null => Shape::Null,
        StackItem::Integer(_) => Shape::Integer(item.as_int().map_or_else(
            |error| format!("integer error {error}"),
            |value| value.to_string(),
        )),
        StackItem::Array(array) => Shape::Compound {
            kind: 'A',
            identity: (0, array.id()),
            stack_references: array.stack_references(),
            children: array.items(),
        },
        StackItem::Struct(structure) => Shape::Compound {
            kind: 'S',
            identity: (1, structure.id()),
            stack_references: structure.stack_references(),
            children: structure.items(),
        },
        StackItem::Map(map) => Shape::Compound {
            kind: 'M',
            identity: (2, map.id()),
            stack_references: map.stack_references(),
            children: map.iter().flat_map(|(key, value)| [key, value]).collect(),
        },
        StackItem::Boolean(value) => Shape::Other(format!("boolean {value}")),
        StackItem::ByteString(bytes) => Shape::Other(format!("bytes {}", bytes.len())),
        _ => Shape::Other("other".to_string()),
    })
}

// ---------------------------------------------------------------------------
// Running a script on the model and on the engine
// ---------------------------------------------------------------------------

/// What one run looked like, instruction by instruction.
#[derive(Debug, Default)]
struct Trace {
    /// The count after every instruction that ran, the faulting one included.
    counts: Vec<usize>,
    /// The evaluation stack after every instruction that succeeded, when
    /// asked for.
    stacks: Vec<String>,
    /// `(index, is the limit check, message)` of the instruction that faulted.
    fault: Option<(usize, bool, String)>,
    /// Count and result stack once the script has ended with the implicit RET.
    final_count: usize,
    result: String,
    halted: bool,
}

fn reference_trace(program: &[Instr], with_stacks: bool) -> Trace {
    let mut model = RefModel::default();
    let mut trace = Trace::default();
    for (index, instr) in program.iter().enumerate() {
        let outcome = model.step(*instr);
        trace.counts.push(model.count);
        match outcome {
            Ok(()) => {
                if with_stacks {
                    trace.stacks.push(model_fingerprint(&model));
                }
            }
            Err(ModelFault::Instruction(message)) => {
                trace.fault = Some((index, false, message));
                return trace;
            }
            Err(ModelFault::Limit(message)) => {
                trace.fault = Some((index, true, message));
                return trace;
            }
        }
    }
    if let Err(fault) = model.implicit_ret() {
        panic!("the implicit RET cannot raise the count: {fault:?}");
    }
    trace.final_count = model.count;
    trace.result = model_fingerprint(&model);
    trace.halted = true;
    trace
}

fn engine_for(script: &[u8]) -> ExecutionEngine {
    let mut engine = ExecutionEngine::new(None);
    engine.set_limits(ExecutionEngineLimits::DEFAULT);
    engine
        .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
        .expect("load test script");
    engine
}

fn is_limit_fault(message: &str) -> bool {
    message.contains("MaxStackSize exceed")
}

/// Steps the engine through `program` one instruction at a time and then lets
/// it finish. The engine is returned so the caller decides where the values
/// it still holds are dropped.
fn engine_trace(program: &[Instr], with_stacks: bool) -> (Trace, ExecutionEngine) {
    let mut engine = engine_for(&assemble(program));
    let mut trace = Trace::default();
    for index in 0..program.len() {
        let outcome = engine.execute_next();
        let signed = engine.reference_counter().signed_count();
        assert!(
            signed >= 0,
            "the count went below zero ({signed}) at {}",
            listing(program, index)
        );
        trace.counts.push(engine.reference_counter().count());
        match outcome {
            Ok(()) if engine.state() == VmState::Fault => {
                let message = "the engine faulted without returning an error".to_string();
                trace.fault = Some((index, false, message));
                return (trace, engine);
            }
            Ok(()) => {
                if with_stacks {
                    let stack: Vec<StackItem> = engine
                        .current_context()
                        .map(|context| context.evaluation_stack().iter().cloned().collect())
                        .unwrap_or_default();
                    trace.stacks.push(engine_fingerprint(&stack));
                }
            }
            Err(error) => {
                let message = error.to_string();
                trace.fault = Some((index, is_limit_fault(&message), message));
                engine.fault(error);
                return (trace, engine);
            }
        }
    }
    let state = engine.execute();
    trace.halted = state == VmState::Halt;
    trace.final_count = engine.reference_counter().count();
    let result: Vec<StackItem> = engine.result_stack().iter().cloned().collect();
    trace.result = engine_fingerprint(&result);
    (trace, engine)
}

/// The engine did, instruction by instruction, what the reference does.
fn assert_same_trace(label: &str, program: &[Instr], reference: &Trace, engine: &Trace) {
    let reference_fault = reference.fault.as_ref().map(|fault| fault.0);
    let engine_fault = engine.fault.as_ref().map(|fault| fault.0);
    let compared = reference.counts.len().min(engine.counts.len());
    for index in 0..compared {
        let faulted = reference_fault == Some(index) || engine_fault == Some(index);
        if faulted {
            break;
        }
        assert_eq!(
            engine.counts[index],
            reference.counts[index],
            "{label}: count after instruction #{index}; {}",
            listing(program, index)
        );
        if let (Some(expected), Some(actual)) =
            (reference.stacks.get(index), engine.stacks.get(index))
        {
            assert_eq!(
                actual,
                expected,
                "{label}: evaluation stack after instruction #{index}; {}",
                listing(program, index)
            );
        }
    }
    assert_eq!(
        engine_fault,
        reference_fault,
        "{label}: faulting instruction; engine {:?}, reference {:?}; {}",
        engine.fault,
        reference.fault,
        listing(program, engine_fault.or(reference_fault).unwrap_or(0))
    );
    match (&reference.fault, &engine.fault) {
        (Some((index, reference_limit, reference_message)), Some((_, engine_limit, message))) => {
            assert_eq!(
                engine_limit, reference_limit,
                "{label}: kind of fault at #{index}: engine {message:?}, reference {reference_message:?}"
            );
            if *reference_limit {
                // Both ran the instruction to its end, so the counts compare.
                assert!(
                    message.contains(reference_message.as_str()),
                    "{label}: engine {message:?}, reference {reference_message:?}"
                );
                assert_eq!(engine.counts[*index], reference.counts[*index], "{label}");
            }
            assert!(!engine.halted, "{label}: the engine halted after a fault");
        }
        _ => {
            assert!(engine.halted, "{label}: the engine did not halt");
            assert_eq!(
                engine.final_count, reference.final_count,
                "{label}: final count"
            );
            assert_eq!(engine.result, reference.result, "{label}: result stack");
        }
    }
}

/// Runs `program` on both and compares them. For use on a large stack.
fn assert_matches_reference(label: &str, program: &[Instr]) -> Trace {
    let with_stacks = program.len() <= 1500;
    let reference = reference_trace(program, with_stacks);
    let (engine, _engine) = engine_trace(program, with_stacks);
    assert_same_trace(label, program, &reference, &engine);
    engine
}

/// The reference trace comes from a large stack; the engine runs, and drops
/// what it built, on a 1 MiB stack.
fn assert_matches_reference_on_small_stack(label: &str, program: Vec<Instr>) -> Trace {
    let model_program = program.clone();
    let reference = on_large_stack(label, move || reference_trace(&model_program, false));
    let owned_label = label.to_string();
    on_small_stack(label, move || {
        let (engine, vm) = engine_trace(&program, false);
        assert_same_trace(&owned_label, &program, &reference, &engine);
        drop(vm);
        engine
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// Halted with this final count.
    Halt(usize),
    /// Faulted at this instruction with this count.
    LimitFault {
        index: usize,
        count: usize,
    },
    OtherFault {
        index: usize,
    },
}

fn outcome(trace: &Trace) -> Outcome {
    match &trace.fault {
        None => {
            assert!(trace.halted, "neither halted nor faulted");
            Outcome::Halt(trace.final_count)
        }
        Some((index, true, _)) => Outcome::LimitFault {
            index: *index,
            count: trace.counts[*index],
        },
        Some((index, false, _)) => Outcome::OtherFault { index: *index },
    }
}

/// The engine and the reference agree on `program`; returns what happened.
/// The engine runs on a 1 MiB stack.
fn chain_outcome(label: &str, program: Vec<Instr>) -> Outcome {
    outcome(&assert_matches_reference_on_small_stack(label, program))
}

/// The engine and the reference agree on `program`; returns the count after
/// every instruction and what happened.
fn counts_and_outcome(label: &str, program: Vec<Instr>) -> (Vec<usize>, Outcome) {
    let owned_label = label.to_string();
    on_large_stack(label, move || {
        let trace = assert_matches_reference(&owned_label, &program);
        let outcome = outcome(&trace);
        (trace.counts, outcome)
    })
}

// ---------------------------------------------------------------------------
// Chains: a value attached to a collection that only its parent holds
// ---------------------------------------------------------------------------

#[test]
fn append_chain_counts_every_link_up_to_the_limit() {
    // Round r ends with r + 2 references and peaks at r + 3 after its TUCK.
    for k in [1, 2, 10, 2044, 2045] {
        let label = format!("APPEND chain of {k}");
        assert_eq!(
            chain_outcome(&label, append_chain(k)),
            Outcome::Halt(k + 2),
            "{label}"
        );
    }
    for k in [2046, 2047, 2100, 5000] {
        let label = format!("APPEND chain of {k}");
        assert_eq!(
            chain_outcome(&label, append_chain(k)),
            Outcome::LimitFault {
                index: 2 + 3 * 2045 + 1,
                count: 2049
            },
            "{label}"
        );
    }
}

#[test]
fn array_setitem_chain_counts_every_link_up_to_the_limit() {
    // Round r ends with r + 3 references and peaks at r + 6 after its PUSH0.
    for k in [1, 2, 10, 2041, 2042] {
        let label = format!("array SETITEM chain of {k}");
        assert_eq!(
            chain_outcome(&label, setitem_array_chain(k)),
            Outcome::Halt(k + 3),
            "{label}"
        );
    }
    for k in [2043, 2044, 2100] {
        let label = format!("array SETITEM chain of {k}");
        assert_eq!(
            chain_outcome(&label, setitem_array_chain(k)),
            Outcome::LimitFault {
                index: 3 + 6 * 2042 + 3,
                count: 2049
            },
            "{label}"
        );
    }
}

#[test]
fn map_setitem_chain_counts_every_key_and_value_up_to_the_limit() {
    // Round r ends with 2r + 2 references and peaks at 2r + 3 after its PUSH0.
    for k in [1, 2, 10, 1021, 1022] {
        let label = format!("map SETITEM chain of {k}");
        assert_eq!(
            chain_outcome(&label, setitem_map_chain(k)),
            Outcome::Halt(2 * k + 2),
            "{label}"
        );
    }
    for k in [1023, 1024, 1100] {
        let label = format!("map SETITEM chain of {k}");
        assert_eq!(
            chain_outcome(&label, setitem_map_chain(k)),
            Outcome::LimitFault {
                index: 2 + 5 * 1022 + 2,
                count: 2049
            },
            "{label}"
        );
    }
}

#[test]
fn append_to_an_array_that_only_its_parent_holds_is_counted() {
    for n in [100, 300, 2044] {
        let label = format!("{n} items appended below a parent");
        assert_eq!(
            chain_outcome(&label, append_into_nested(n)),
            Outcome::Halt(n + 3),
            "{label}"
        );
    }
    // P, A and the new array with its 2045 items while it is on the stack.
    assert_eq!(
        chain_outcome(
            "2045 items appended below a parent",
            append_into_nested(2045)
        ),
        Outcome::LimitFault {
            index: 7,
            count: 2049
        }
    );
}

/// One instruction releases a chain of two thousand arrays and activates it
/// again, on a 1 MiB stack.
#[test]
fn deep_chain_is_released_and_activated_within_one_instruction() {
    use OpCode::*;

    // STLOC0 pops A0, which releases A0..A2000, and stores it in the slot,
    // which activates them again.
    let program = concat(&[
        &[InitSlot(1, 0)],
        &append_chain(2000),
        &ops(&[DROP, STLOC0, LDLOC0, DROP]),
    ]);
    assert_eq!(program.len(), 6007);
    let trace = assert_matches_reference_on_small_stack("chain through a local", program);
    assert_eq!(trace.counts[6002..], [2003, 2002, 2001, 2002, 2001]);
    assert_eq!(outcome(&trace), Outcome::Halt(0));

    let program = concat(&[&append_chain(2000), &ops(&[DROP, DROP])]);
    let trace = assert_matches_reference_on_small_stack("chain dropped", program);
    assert_eq!(trace.counts[6001..], [2002, 2001, 0]);
    assert_eq!(outcome(&trace), Outcome::Halt(0));
}

// ---------------------------------------------------------------------------
// REMOVE, CLEARITEMS, POPITEM
// ---------------------------------------------------------------------------

#[test]
fn remove_releases_what_the_array_held() {
    use OpCode::*;

    let (counts, outcome) = counts_and_outcome(
        "REMOVE from an array",
        packed_child_release(&[Int(0), Op(REMOVE)]),
    );
    assert_eq!(counts, [1, 301, 302, 302, 303, 304, 1]);
    assert_eq!(outcome, Outcome::Halt(1));

    let (counts, outcome) = counts_and_outcome(
        "REMOVE from an array, then DROP",
        packed_child_release(&[Int(0), Op(REMOVE), Op(DROP)]),
    );
    assert_eq!(counts, [1, 301, 302, 302, 303, 304, 1, 0]);
    assert_eq!(outcome, Outcome::Halt(0));
}

#[test]
fn remove_releases_the_key_and_the_value_of_a_map() {
    use OpCode::*;

    let program = vec![
        Int(300),
        Op(NEWARRAY),
        Int(5),
        Int(1),
        Op(PACKMAP),
        Op(DUP),
        Int(5),
        Op(REMOVE),
    ];
    let (counts, outcome) = counts_and_outcome("REMOVE from a map", program);
    assert_eq!(counts, [1, 301, 302, 303, 303, 304, 305, 1]);
    assert_eq!(outcome, Outcome::Halt(1));
}

#[test]
fn clearitems_releases_what_the_array_held() {
    use OpCode::*;

    let (counts, outcome) =
        counts_and_outcome("CLEARITEMS", packed_child_release(&[Op(CLEARITEMS)]));
    assert_eq!(counts, [1, 301, 302, 302, 303, 1]);
    assert_eq!(outcome, Outcome::Halt(1));

    let (counts, outcome) = counts_and_outcome(
        "CLEARITEMS, then DROP",
        packed_child_release(&[Op(CLEARITEMS), Op(DROP)]),
    );
    assert_eq!(counts, [1, 301, 302, 302, 303, 1, 0]);
    assert_eq!(outcome, Outcome::Halt(0));
}

#[test]
fn popitem_moves_the_reference_from_the_array_to_the_stack() {
    use OpCode::*;

    let (counts, outcome) = counts_and_outcome("POPITEM", packed_child_release(&[Op(POPITEM)]));
    assert_eq!(counts, [1, 301, 302, 302, 303, 302]);
    assert_eq!(outcome, Outcome::Halt(302));

    let (counts, outcome) = counts_and_outcome(
        "POPITEM, then DROP DROP",
        packed_child_release(&[Op(POPITEM), Op(DROP), Op(DROP)]),
    );
    assert_eq!(counts, [1, 301, 302, 302, 303, 302, 1, 0]);
    assert_eq!(outcome, Outcome::Halt(0));
}

// ---------------------------------------------------------------------------
// Collections that hold themselves
// ---------------------------------------------------------------------------

/// Runs a script that stores a collection into itself on a worker thread: a
/// script that never returns fails the test instead of hanging the run.
fn self_reference_outcome(label: &str, program: Vec<Instr>) -> (Outcome, String) {
    let owned_label = label.to_string();
    run_on_thread(label, SMALL_STACK, SELF_REFERENCE_LIMIT, move || {
        let trace = assert_matches_reference(&owned_label, &program);
        (outcome(&trace), trace.result)
    })
}

#[test]
fn array_appended_to_itself_and_dropped_stays_counted() {
    use OpCode::*;

    let program = ops(&[NEWARRAY0, DUP, DUP, APPEND, DROP]);
    let (outcome, result) = self_reference_outcome("NEWARRAY0 DUP DUP APPEND DROP", program);
    assert_eq!(outcome, Outcome::Halt(1));
    assert_eq!(result, "");
}

#[test]
fn array_appended_to_itself_without_a_stack_reference_is_not_counted() {
    use OpCode::*;

    let program = ops(&[NEWARRAY0, DUP, APPEND]);
    let (outcome, result) = self_reference_outcome("NEWARRAY0 DUP APPEND", program);
    assert_eq!(outcome, Outcome::Halt(0));
    assert_eq!(result, "");
}

#[test]
fn array_stored_into_its_own_slot() {
    use OpCode::*;

    let program = vec![
        Int(1),
        Op(NEWARRAY),
        Op(DUP),
        Op(DUP),
        Int(0),
        Op(ROT),
        Op(SETITEM),
    ];
    let (outcome, result) = self_reference_outcome("array SETITEM of itself", program);
    assert_eq!(outcome, Outcome::Halt(2));
    assert_eq!(result, "A0x2[ @0 ] ");
}

#[test]
fn map_stored_as_its_own_value() {
    use OpCode::*;

    let program = vec![Op(NEWMAP), Op(DUP), Op(DUP), Int(0), Op(ROT), Op(SETITEM)];
    let (outcome, result) = self_reference_outcome("map SETITEM of itself", program);
    assert_eq!(outcome, Outcome::Halt(3));
    assert_eq!(result, "M0x2[ 0 @0 ] ");
}

#[test]
fn two_arrays_appended_to_each_other() {
    use OpCode::*;

    let program = ops(&[
        NEWARRAY0, NEWARRAY0, OVER, OVER, APPEND, SWAP, OVER, OVER, APPEND,
    ]);
    let (outcome, result) = self_reference_outcome("two arrays, each in the other", program);
    assert_eq!(outcome, Outcome::Halt(4));
    assert_eq!(result, "A0x2[ A1x2[ @0 ] ] @1 ");
}

#[test]
fn struct_appended_to_itself_holds_a_clone() {
    use OpCode::*;

    let program = ops(&[NEWSTRUCT0, DUP, DUP, APPEND]);
    let (outcome, result) = self_reference_outcome("NEWSTRUCT0 DUP DUP APPEND", program);
    assert_eq!(outcome, Outcome::Halt(2));
    assert_eq!(result, "S0x1[ S1x1[ ] ] ");

    let program = ops(&[NEWSTRUCT0, DUP, DUP, APPEND, DROP]);
    let (outcome, _) = self_reference_outcome("NEWSTRUCT0 DUP DUP APPEND DROP", program);
    assert_eq!(outcome, Outcome::Halt(0));
}

/// The collection methods and the counter return when a collection is its
/// own sub-item.
#[test]
fn collections_hold_themselves_through_the_api() {
    run_on_thread(
        "collections holding themselves",
        SMALL_STACK,
        SELF_REFERENCE_LIMIT,
        || {
            let array = Array::new(vec![StackItem::Null]);
            let array_item = StackItem::Array(array.clone());
            array.set(0, array_item.clone()).expect("array set");
            array.push(array_item.clone()).expect("array push");

            let structure = Struct::new(vec![StackItem::Null]);
            let struct_item = StackItem::Struct(structure.clone());
            structure.set(0, struct_item.clone()).expect("struct set");
            structure.push(struct_item.clone()).expect("struct push");

            let map = Map::new(VmOrderedDictionary::new());
            let map_item = StackItem::Map(map.clone());
            map.set(StackItem::from_int(1), map_item.clone())
                .expect("map set");

            // item + its sub-items, each sub-item being the item again.
            for (item, references, sub_items) in
                [(&array_item, 3, 2), (&struct_item, 3, 2), (&map_item, 2, 2)]
            {
                let counter = ReferenceCounter::new();
                counter.add_stack_reference(item, 1);
                assert_eq!(counter.count(), 1 + sub_items);
                counter.remove_stack_reference(item);
                assert_eq!(counter.count(), sub_items);
                assert_eq!(counter.signed_count(), sub_items as isize);
                let held = match item {
                    StackItem::Array(array) => array.stack_references(),
                    StackItem::Struct(structure) => structure.stack_references(),
                    StackItem::Map(map) => map.stack_references(),
                    _ => unreachable!("only collections are tested"),
                };
                assert_eq!(held, references - 1);
            }
        },
    );
}

// ---------------------------------------------------------------------------
// Nothing is collected
// ---------------------------------------------------------------------------

#[test]
fn abandoned_cycles_stay_counted_until_the_limit() {
    // Every round leaves one more reference behind and peaks three above
    // where it started.
    let trace = assert_matches_reference_on_small_stack("2046 cycles", abandoned_cycles(2046));
    assert_eq!(outcome(&trace), Outcome::Halt(2046));
    assert_eq!(trace.result, "", "nothing is left on the stack");

    let trace = assert_matches_reference_on_small_stack("2047 cycles", abandoned_cycles(2047));
    assert_eq!(
        outcome(&trace),
        Outcome::LimitFault {
            index: 5 * 2046 + 2,
            count: 2049
        }
    );
}

// ---------------------------------------------------------------------------
// The check after the instruction
// ---------------------------------------------------------------------------

struct Finished {
    state: VmState,
    count: usize,
    fault: Option<String>,
    result: Vec<StackItem>,
}

/// Runs `script` to its end one instruction at a time, so that the error of
/// a faulting instruction can be reported.
fn run_to_end(script: Vec<u8>, prepare: impl FnOnce(&mut ExecutionEngine)) -> Finished {
    let mut engine = ExecutionEngine::new(None);
    engine.set_limits(ExecutionEngineLimits::DEFAULT);
    prepare(&mut engine);
    engine
        .load_script(Script::new_relaxed(script), -1, 0)
        .expect("load test script");
    let mut fault = None;
    while engine.state() != VmState::Halt && engine.state() != VmState::Fault {
        if let Err(error) = engine.execute_next() {
            fault = Some(error.to_string());
            engine.fault(error);
        }
    }
    Finished {
        state: engine.state(),
        count: engine.reference_counter().count(),
        fault,
        result: engine.result_stack().iter().cloned().collect(),
    }
}

/// Stands in for an instruction that has pushed a value before it raises a
/// catchable engine exception. No instruction of Neo.VM itself does that: they
/// pop at least two operands before they can raise, and the exception that is
/// pushed for the handler never brings the count above where it started.
fn push_then_raise(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(7))?;
    Err(VmError::CatchableException {
        message: "raised by the test handler".to_string(),
    })
}

/// `PUSH1×pushes TRY { NOP } CATCH { DROP } RET`.
fn guarded_nop(pushes: usize) -> Vec<u8> {
    let mut script = vec![OpCode::PUSH1.byte(); pushes];
    script.extend([
        OpCode::TRY.byte(),
        6, // catch block
        0, // no finally block
        OpCode::NOP.byte(),
        OpCode::ENDTRY.byte(),
        5,
        OpCode::DROP.byte(), // catch: the exception
        OpCode::ENDTRY.byte(),
        2,
        OpCode::RET.byte(),
    ]);
    script
}

#[test]
fn limit_is_checked_after_a_caught_engine_exception() {
    let raise_on_nop = |engine: &mut ExecutionEngine| {
        engine
            .jump_table_mut()
            .set_handler(OpCode::NOP, push_then_raise);
    };

    // 2046 + the pushed value + the exception = 2048.
    let run = run_to_end(guarded_nop(2046), raise_on_nop);
    assert_eq!(run.state, VmState::Halt, "{:?}", run.fault);
    assert_eq!(run.count, 2047);
    assert_eq!(run.result.len(), 2047);

    // 2047 + the pushed value + the exception = 2049, which the catch block
    // would bring back to 2048 with its DROP.
    let run = run_to_end(guarded_nop(2047), raise_on_nop);
    assert_eq!(run.state, VmState::Fault);
    let fault = run.fault.expect("the limit check reports an error");
    assert!(fault.contains("MaxStackSize exceed: 2049/2048"), "{fault}");
}

#[test]
fn caught_engine_exception_is_pushed_and_counted() {
    // TRY { NEWARRAY0 PUSH0 PICKITEM } CATCH { } RET
    let script = vec![
        OpCode::TRY.byte(),
        8,
        0,
        OpCode::NEWARRAY0.byte(),
        OpCode::PUSH0.byte(),
        OpCode::PICKITEM.byte(),
        OpCode::ENDTRY.byte(),
        4,
        OpCode::ENDTRY.byte(),
        2,
        OpCode::RET.byte(),
    ];
    let run = run_to_end(script, |_| {});
    assert_eq!(run.state, VmState::Halt, "{:?}", run.fault);
    assert_eq!(run.result.len(), 1, "the exception is the result");
    assert_eq!(run.count, 1);
}

fn ret_that_pushes(engine: &mut ExecutionEngine, _instruction: &Instruction) -> VmResult<()> {
    engine.push(StackItem::from_int(1))
}

/// The RET that ends a script without one is an instruction like any other:
/// it is dispatched through the jump table and followed by the limit check.
#[test]
fn implicit_ret_runs_as_an_instruction() {
    let script = vec![OpCode::PUSH1.byte(); 2048];
    let run = run_to_end(script.clone(), |_| {});
    assert_eq!(run.state, VmState::Halt, "{:?}", run.fault);
    assert_eq!(run.count, 2048);

    let run = run_to_end(script, |engine| {
        engine
            .jump_table_mut()
            .set_handler(OpCode::RET, ret_that_pushes);
    });
    assert_eq!(run.state, VmState::Fault);
    let fault = run.fault.expect("the limit check reports an error");
    assert!(fault.contains("MaxStackSize exceed: 2049/2048"), "{fault}");
}

#[test]
fn collection_size_and_item_count_follow_the_configured_limit() {
    use OpCode::*;

    let with_limit = |max_stack_size: u32, size: usize| {
        run_to_end(assemble(&[Int(size), Op(NEWARRAY)]), move |engine| {
            let mut limits = ExecutionEngineLimits::DEFAULT;
            limits.max_stack_size = max_stack_size;
            engine.set_limits(limits);
        })
    };

    let run = with_limit(4096, 3000);
    assert_eq!(run.state, VmState::Halt, "{:?}", run.fault);
    assert_eq!(run.count, 3001);

    let run = with_limit(4096, 4097);
    assert_eq!(run.state, VmState::Fault);
    let fault = run.fault.expect("NEWARRAY reports an error");
    assert!(
        !is_limit_fault(&fault),
        "NEWARRAY rejects the size: {fault}"
    );

    let run = with_limit(100, 99);
    assert_eq!(run.state, VmState::Halt, "{:?}", run.fault);
    assert_eq!(run.count, 100);

    // The size is allowed; the array and its hundred items are not.
    let run = with_limit(100, 100);
    assert_eq!(run.state, VmState::Fault);
    let fault = run.fault.expect("the limit check reports an error");
    assert!(fault.contains("MaxStackSize exceed: 101/100"), "{fault}");
}

// ---------------------------------------------------------------------------
// Differential: fixed scripts
// ---------------------------------------------------------------------------

/// A 600-item struct held `copies` times by another struct, which APPEND then
/// has to clone: `copies + 600 × copies` sub-items against a budget of 2047.
fn struct_clone_budget(copies: usize) -> Vec<Instr> {
    use OpCode::*;
    let mut program = vec![Int(600), Op(NEWSTRUCT)];
    program.extend(std::iter::repeat_n(Op(DUP), copies - 1));
    program.extend([Int(copies), Op(PACKSTRUCT)]);
    program.extend(ops(&[NEWARRAY0, TUCK, SWAP, APPEND]));
    program
}

fn fixed_scripts() -> Vec<(&'static str, Vec<Instr>)> {
    use OpCode::*;
    vec![
        ("APPEND chain of 150", append_chain(150)),
        ("array SETITEM chain of 100", setitem_array_chain(100)),
        ("map SETITEM chain of 100", setitem_map_chain(100)),
        ("struct nested once", struct_chain(1)),
        ("struct nested 5 deep", struct_chain(5)),
        ("struct nested 30 deep", struct_chain(30)),
        ("100 items appended below a parent", append_into_nested(100)),
        (
            "REMOVE of a packed child",
            packed_child_release(&[Int(0), Op(REMOVE), Op(DROP)]),
        ),
        (
            "CLEARITEMS of a packed child",
            packed_child_release(&[Op(CLEARITEMS), Op(DROP)]),
        ),
        (
            "POPITEM of a packed child",
            packed_child_release(&[Op(POPITEM), Op(DROP), Op(DROP)]),
        ),
        ("50 abandoned cycles", abandoned_cycles(50)),
        (
            "array holding itself",
            ops(&[NEWARRAY0, DUP, DUP, APPEND, DUP, DUP, APPEND, DROP]),
        ),
        (
            "two arrays holding each other, dropped",
            ops(&[
                NEWARRAY0, NEWARRAY0, OVER, OVER, APPEND, SWAP, OVER, OVER, APPEND, DROP, DROP,
            ]),
        ),
        (
            "map packed, then keys, values and UNPACK",
            vec![
                Int(7),
                Int(1),
                Op(NEWARRAY0),
                Int(2),
                Int(2),
                Op(PACKMAP),
                Op(DUP),
                Op(KEYS),
                Op(OVER),
                Op(VALUES),
                Op(ROT),
                Op(UNPACK),
            ],
        ),
        (
            "map packed with a repeated key",
            vec![Int(7), Int(1), Op(NEWARRAY0), Int(1), Int(2), Op(PACKMAP)],
        ),
        (
            "array reversed, read and measured",
            vec![
                Int(1),
                Int(2),
                Int(3),
                Int(3),
                Op(PACK),
                Op(DUP),
                Op(REVERSEITEMS),
                Op(DUP),
                Int(0),
                Op(PICKITEM),
                Op(OVER),
                Op(SIZE),
            ],
        ),
        (
            "VALUES clones the structs it copies",
            vec![
                Op(NEWSTRUCT0),
                Op(DUP),
                Int(5),
                Op(APPEND),
                Int(1),
                Op(PACK),
                Op(DUP),
                Op(VALUES),
            ],
        ),
        (
            "array of collections unpacked",
            vec![
                Int(2),
                Op(NEWARRAY),
                Op(NEWARRAY0),
                Op(NEWMAP),
                Int(3),
                Op(PACK),
                Op(DUP),
                Op(UNPACK),
            ],
        ),
        (
            "struct packed from the stack",
            vec![
                Op(NEWSTRUCT0),
                Op(DUP),
                Int(2),
                Op(PACKSTRUCT),
                Op(DUP),
                Int(0),
                Op(PICKITEM),
                Int(9),
                Op(APPEND),
            ],
        ),
        (
            "static fields",
            vec![
                InitSSlot(2),
                Op(NEWARRAY0),
                Op(DUP),
                Op(STSFLD0),
                Op(DUP),
                Op(LDSFLD0),
                Op(APPEND),
                Op(LDSFLD0),
                Op(STSFLD1),
                Op(DROP),
                Op(PUSHNULL),
                Op(STSFLD0),
            ],
        ),
        (
            "arguments and locals",
            vec![
                Int(4),
                Op(NEWARRAY0),
                InitSlot(2, 2),
                Op(LDARG0),
                Op(LDARG1),
                Op(APPEND),
                Op(LDARG0),
                Op(STLOC0),
                Op(PUSHNULL),
                Op(STARG0),
                Op(LDLOC0),
                Op(DUP),
                Op(STLOC1),
                Int(300),
                Op(NEWARRAY),
                Op(APPEND),
            ],
        ),
        (
            "slot of an array that only its parent holds, replaced twice",
            vec![
                Int(1),
                Op(NEWARRAY),
                Int(1),
                Op(PACK),
                Op(DUP),
                Int(0),
                Op(PICKITEM),
                Int(0),
                Int(300),
                Op(NEWARRAY),
                Op(SETITEM),
                Op(DUP),
                Int(0),
                Op(PICKITEM),
                Int(0),
                Op(PUSHNULL),
                Op(SETITEM),
            ],
        ),
        (
            "map value set, replaced and removed",
            vec![
                Op(NEWMAP),
                Op(DUP),
                Int(1),
                Op(NEWARRAY0),
                Op(SETITEM),
                Op(DUP),
                Int(1),
                Int(300),
                Op(NEWARRAY),
                Op(SETITEM),
                Op(DUP),
                Int(1),
                Op(PUSHNULL),
                Op(SETITEM),
                Op(DUP),
                Int(1),
                Op(REMOVE),
                Op(DUP),
                Int(1),
                Op(REMOVE),
            ],
        ),
        (
            "map cleared",
            vec![
                Int(300),
                Op(NEWARRAY),
                Int(1),
                Op(NEWMAP),
                Int(2),
                Int(2),
                Op(PACKMAP),
                Op(DUP),
                Op(CLEARITEMS),
            ],
        ),
        (
            "stack cleared",
            vec![
                Op(NEWARRAY0),
                Op(DUP),
                Op(DUP),
                Op(APPEND),
                Int(3),
                Op(NEWARRAY),
                Op(CLEAR),
            ],
        ),
        (
            "stack shuffled",
            vec![
                Op(NEWARRAY0),
                Op(NEWMAP),
                Int(3),
                Op(ROT),
                Op(TUCK),
                Op(NIP),
                Int(2),
                Op(PICK),
                Op(SWAP),
                Op(OVER),
            ],
        ),
    ]
}

#[test]
fn fixed_scripts_count_like_the_reference() {
    on_large_stack("fixed scripts", || {
        for (label, program) in fixed_scripts() {
            let trace = assert_matches_reference(label, &program);
            assert!(trace.halted, "{label}: {:?}", trace.fault);
        }
    });
}

#[test]
fn faults_happen_where_the_reference_faults() {
    use OpCode::*;

    let other = |index| Outcome::OtherFault { index };
    let scripts: Vec<(&'static str, Vec<Instr>, Outcome)> = vec![
        (
            "PICKITEM past the end of an array",
            vec![Op(NEWARRAY0), Int(0), Op(PICKITEM)],
            other(2),
        ),
        (
            "PICKITEM of a missing key",
            vec![Op(NEWMAP), Int(1), Op(PICKITEM)],
            other(2),
        ),
        (
            "REMOVE past the end of an array",
            vec![Int(1), Op(NEWARRAY), Int(1), Op(REMOVE)],
            other(3),
        ),
        (
            "SETITEM past the end of an array",
            vec![Op(NEWARRAY0), Int(0), Int(1), Op(SETITEM)],
            other(3),
        ),
        (
            "POPITEM of an empty array",
            vec![Op(NEWARRAY0), Op(POPITEM)],
            other(1),
        ),
        (
            "APPEND to a map",
            vec![Op(NEWMAP), Int(1), Op(APPEND)],
            other(2),
        ),
        (
            "PACK of more than the stack holds",
            vec![Int(1), Int(5), Op(PACK)],
            other(2),
        ),
        (
            "NEWARRAY above MaxStackSize",
            vec![Int(2049), Op(NEWARRAY)],
            other(1),
        ),
        (
            "NEWSTRUCT above MaxStackSize",
            vec![Int(2049), Op(NEWSTRUCT)],
            other(1),
        ),
        (
            "NEWARRAY of MaxStackSize items",
            vec![Int(2048), Op(NEWARRAY)],
            Outcome::LimitFault {
                index: 1,
                count: 2049,
            },
        ),
        (
            "NEWARRAY of MaxStackSize - 1 items",
            vec![Int(2047), Op(NEWARRAY)],
            Outcome::Halt(2048),
        ),
        (
            "INITSSLOT twice",
            vec![InitSSlot(1), InitSSlot(1)],
            other(1),
        ),
        ("LDLOC0 without INITSLOT", vec![Op(LDLOC0)], other(0)),
        (
            "struct clone within its budget",
            struct_clone_budget(3),
            Outcome::Halt(1805),
        ),
        (
            "struct clone beyond its budget",
            struct_clone_budget(4),
            other(10),
        ),
    ];

    on_large_stack("faulting scripts", move || {
        for (label, program, expected) in scripts {
            let trace = assert_matches_reference(label, &program);
            assert_eq!(outcome(&trace), expected, "{label}");
        }
    });
}

// ---------------------------------------------------------------------------
// Differential: scripts generated from fixed seeds
// ---------------------------------------------------------------------------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..bound`.
    fn below(&mut self, bound: usize) -> usize {
        ((self.next() >> 33) % bound as u64) as usize
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
}

/// Writes a script while running it on the reference model, so that every
/// instruction it emits finds the operands it needs. The script ends at the
/// first instruction the model faults on.
struct Generator {
    rng: Rng,
    model: RefModel,
    program: Vec<Instr>,
    stopped: bool,
    /// Allows collections of a few hundred items, which reach the limit.
    heavy: bool,
}

impl Generator {
    fn emit(&mut self, instr: Instr) {
        if self.stopped {
            return;
        }
        self.program.push(instr);
        self.stopped = self.model.step(instr).is_err();
    }

    fn op(&mut self, opcode: OpCode) {
        self.emit(Op(opcode));
    }

    fn int(&mut self, bound: usize) {
        let value = self.rng.below(bound);
        self.emit(Int(value));
    }

    fn depth(&self) -> usize {
        self.model.stack.len()
    }

    /// `(id, kind, length)` of the collection `from_top` items below the top.
    fn compound_at(&self, from_top: usize) -> Option<(usize, Kind, usize)> {
        match self.model.peek(from_top) {
            Ok(RItem::Compound(id)) => Some((id, self.model.heap[id].kind, self.model.len_of(id))),
            _ => None,
        }
    }

    fn array_at(&self, from_top: usize) -> Option<(usize, Kind, usize)> {
        self.compound_at(from_top)
            .filter(|(_, kind, _)| *kind != Kind::Map)
    }

    /// Pushes one value: a new one or one that is on the stack already,
    /// which may be the collection the next instruction works on.
    fn value(&mut self) {
        use OpCode::*;
        match self.rng.below(10) {
            0..=2 => self.int(20),
            3 => self.op(PUSHNULL),
            4 => self.op(NEWARRAY0),
            5 => self.op(NEWSTRUCT0),
            6 => self.op(NEWMAP),
            _ if self.depth() > 0 => {
                let bound = self.depth().min(6);
                self.int(bound);
                self.op(PICK);
            }
            _ => self.int(20),
        }
    }

    fn existing_key(&mut self, id: usize) -> Option<usize> {
        let keys: Vec<usize> = self.model.heap[id]
            .items
            .iter()
            .step_by(2)
            .filter_map(|key| match key {
                RItem::Int(value) => usize::try_from(*value).ok(),
                _ => None,
            })
            .collect();
        if keys.is_empty() {
            return None;
        }
        Some(keys[self.rng.below(keys.len())])
    }

    /// A key that the map has, more often than not.
    fn map_key(&mut self, id: usize) -> usize {
        match self.existing_key(id) {
            Some(key) if self.rng.chance(60) => key,
            _ => self.rng.below(6),
        }
    }

    /// An index or key that `id` has, if it has one.
    fn position_in(&mut self, id: usize, kind: Kind, len: usize) -> Option<usize> {
        match kind {
            Kind::Map => self.existing_key(id),
            Kind::Array | Kind::Struct if len > 0 => Some(self.rng.below(len)),
            Kind::Array | Kind::Struct => None,
        }
    }

    fn prologue(&mut self) {
        if self.rng.chance(50) {
            let count = 1 + self.rng.below(3);
            self.emit(InitSSlot(count as u8));
        }
        if self.rng.chance(60) {
            let arguments = self.rng.below(3);
            let locals = self.rng.below(4);
            if arguments + locals > 0 {
                for _ in 0..arguments {
                    self.value();
                }
                self.emit(InitSlot(locals as u8, arguments as u8));
            }
        }
    }

    fn slot_instruction(&mut self) {
        let slots = [
            (&self.model.static_fields, LOAD_STATIC, STORE_STATIC),
            (&self.model.local_variables, LOAD_LOCAL, STORE_LOCAL),
            (&self.model.arguments, LOAD_ARGUMENT, STORE_ARGUMENT),
        ];
        let usable: Vec<(usize, [OpCode; 3], [OpCode; 3])> = slots
            .into_iter()
            .filter_map(|(slot, load, store)| slot.as_ref().map(|items| (items.len(), load, store)))
            .collect();
        if usable.is_empty() {
            self.value();
            return;
        }
        let (len, load, store) = usable[self.rng.below(usable.len())];
        let index = self.rng.below(len);
        if self.depth() > 0 && self.rng.chance(50) {
            self.op(store[index]);
        } else {
            self.op(load[index]);
        }
    }

    /// An instruction that faults in the reference.
    fn faulting_instruction(&mut self) {
        use OpCode::*;
        let depth = self.depth();
        let program = match self.rng.below(5) {
            0 => vec![Op(NEWARRAY0), Int(0), Op(PICKITEM)],
            1 => vec![Op(NEWARRAY0), Op(POPITEM)],
            2 => vec![Int(1), Op(NEWARRAY), Int(3), Op(REMOVE)],
            3 => vec![Op(NEWMAP), Int(1), Op(PICKITEM)],
            _ => vec![Int(depth + 5), Op(PACK)],
        };
        for instr in program {
            self.emit(instr);
        }
    }

    fn step(&mut self) {
        use OpCode::*;

        let depth = self.depth();
        if depth > 10 && self.rng.chance(40) {
            self.op(DROP);
            return;
        }
        let top = self.compound_at(0);
        let top_array = self.array_at(0);
        match self.rng.below(100) {
            0..=11 => self.value(),
            12..=15 => {
                let bound = if self.heavy && self.rng.chance(60) {
                    900
                } else {
                    4
                };
                self.int(bound);
                let opcode = if self.rng.chance(70) {
                    NEWARRAY
                } else {
                    NEWSTRUCT
                };
                self.op(opcode);
            }
            16..=19 if depth >= 1 => self.op(DUP),
            20..=22 if depth >= 2 => self.op(OVER),
            23..=25 if depth >= 2 => self.op(TUCK),
            26..=28 if depth >= 2 => self.op(SWAP),
            29..=30 if depth >= 3 => self.op(ROT),
            31..=35 if depth >= 1 => self.op(DROP),
            36..=37 if depth >= 2 => self.op(NIP),
            38..=42 => {
                self.int(depth.min(4) + 1);
                let opcode = if self.rng.chance(70) {
                    PACK
                } else {
                    PACKSTRUCT
                };
                self.op(opcode);
            }
            43..=46 => {
                let pairs = 1 + self.rng.below(2);
                for _ in 0..pairs {
                    self.value();
                    self.int(4);
                }
                self.emit(Int(pairs));
                self.op(PACKMAP);
            }
            47..=49 => match top {
                Some((_, _, len)) if len <= 6 => {
                    if self.rng.chance(50) {
                        self.op(DUP);
                    }
                    self.op(UNPACK);
                }
                _ => self.value(),
            },
            50..=54 => match top {
                Some((id, kind, len)) => match self.position_in(id, kind, len) {
                    Some(position) => {
                        if self.rng.chance(70) {
                            self.op(DUP);
                        }
                        self.emit(Int(position));
                        self.op(PICKITEM);
                    }
                    None => self.value(),
                },
                None => self.value(),
            },
            55..=66 => {
                let below_top = self.array_at(1);
                match self.rng.below(4) {
                    // The collection itself.
                    0 if top_array.is_some() => {
                        self.op(DUP);
                        self.op(DUP);
                        self.op(APPEND);
                    }
                    2 if below_top.is_some() => {
                        self.op(OVER);
                        self.op(OVER);
                        self.op(APPEND);
                    }
                    // Uses up what may be the only stack reference.
                    3 if below_top.is_some() => self.op(APPEND),
                    _ if top_array.is_some() => {
                        self.op(DUP);
                        self.value();
                        self.op(APPEND);
                    }
                    _ => self.value(),
                }
            }
            67..=76 => match top {
                Some((id, kind, len)) if kind == Kind::Map || len > 0 => {
                    if self.rng.chance(80) {
                        self.op(DUP);
                    }
                    let key = match kind {
                        Kind::Map => self.map_key(id),
                        Kind::Array | Kind::Struct => self.rng.below(len),
                    };
                    self.emit(Int(key));
                    self.value();
                    self.op(SETITEM);
                }
                _ => self.value(),
            },
            77..=81 => match top {
                Some((id, kind, len)) if kind == Kind::Map || len > 0 => {
                    if self.rng.chance(70) {
                        self.op(DUP);
                    }
                    let key = match kind {
                        Kind::Map => self.map_key(id),
                        Kind::Array | Kind::Struct => self.rng.below(len),
                    };
                    self.emit(Int(key));
                    self.op(REMOVE);
                }
                _ => self.value(),
            },
            82..=84 if top.is_some() => {
                if self.rng.chance(70) {
                    self.op(DUP);
                }
                self.op(CLEARITEMS);
            }
            85..=88 => match top_array {
                Some((_, _, len)) if len > 0 => {
                    if self.rng.chance(70) {
                        self.op(DUP);
                    }
                    self.op(POPITEM);
                }
                _ => self.value(),
            },
            89..=94 => self.slot_instruction(),
            95..=97 => match top {
                Some((_, kind, _)) => {
                    let choices: &[OpCode] = match kind {
                        Kind::Map => &[KEYS, VALUES, SIZE],
                        Kind::Array | Kind::Struct => &[VALUES, SIZE, REVERSEITEMS],
                    };
                    let opcode = choices[self.rng.below(choices.len())];
                    self.op(DUP);
                    self.op(opcode);
                }
                None => self.value(),
            },
            98 if self.rng.chance(10) => self.op(CLEAR),
            98 if self.rng.chance(10) => self.faulting_instruction(),
            _ => self.value(),
        }
    }
}

fn random_script(seed: u64, heavy: bool, steps: usize) -> Vec<Instr> {
    let mut generator = Generator {
        rng: Rng::new(seed),
        model: RefModel::default(),
        program: Vec::new(),
        stopped: false,
        heavy,
    };
    generator.prologue();
    for _ in 0..steps {
        if generator.stopped {
            break;
        }
        generator.step();
    }
    generator.program
}

#[test]
fn random_scripts_count_like_the_reference() {
    let (halted, limit_faults, other_faults, instructions) =
        on_large_stack("random scripts", || {
            let mut halted = 0_usize;
            let mut limit_faults = 0_usize;
            let mut other_faults = 0_usize;
            let mut instructions = 0_usize;
            for (heavy, seeds, steps) in [(false, 400_u64, 200_usize), (true, 150, 300)] {
                for seed in 1..=seeds {
                    let seed = if heavy { 1_000_000 + seed } else { seed };
                    let program = random_script(seed, heavy, steps);
                    let label =
                        format!("random script, seed {seed}, {steps} steps, heavy: {heavy}");
                    let trace = assert_matches_reference(&label, &program);
                    instructions += trace.counts.len();
                    match outcome(&trace) {
                        Outcome::Halt(_) => halted += 1,
                        Outcome::LimitFault { .. } => limit_faults += 1,
                        Outcome::OtherFault { .. } => other_faults += 1,
                    }
                }
            }
            (halted, limit_faults, other_faults, instructions)
        });
    println!(
        "random scripts: {halted} halted, {limit_faults} faulted on the limit, \
         {other_faults} faulted in an instruction, {instructions} instructions compared"
    );
    // The batch has to reach all three endings to mean anything.
    assert!(halted > 0, "no generated script halted");
    assert!(limit_faults > 0, "no generated script reached the limit");
    assert!(
        other_faults > 0,
        "no generated script faulted in an instruction"
    );
}
