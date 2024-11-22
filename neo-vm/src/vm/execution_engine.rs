use std::{
    cell::{RefCell},
    convert::TryInto,
    fmt::Error,
    ops::Neg,
    rc::Rc,
};
use num_bigint::BigInt;
use num_traits::FromBytes;
use neo_base::math::I256;
use crate::exception::exception_handling_context::ExceptionHandlingContext;
use crate::exception::exception_handling_state::ExceptionHandlingState;
use crate::References;
use crate::vm::{EvaluationStack, ExecutionContext, ExecutionEngineLimits, Instruction, OpCode, Script, SharedStates, VMError, VMState};
use crate::stack_item::{SharedItem, StackItem};
use crate::stack_item::StackItem::Integer;
use crate::vm::slots::Slots;

/// Represents the VM used to execute the script.
#[derive(Clone)]
pub struct ExecutionEngine {
    /// Restrictions on the VM.
    pub limits: ExecutionEngineLimits,

    /// Used for reference counting of objects in the VM.
    pub reference_counter: Rc<RefCell<References>>,

    /// The invocation stack of the VM.
    pub invocation_stack: Vec<Rc<RefCell<ExecutionContext>>>,

    /// The top frame of the invocation stack.
    pub current_context: Option<Rc<RefCell<ExecutionContext>>>,

    /// The bottom frame of the invocation stack.
    pub entry_context: Option<Rc<RefCell<ExecutionContext>>>,

    /// The stack to store the return values.
    pub result_stack: Rc<RefCell<EvaluationStack>>,

    /// The VM object representing the uncaught exception.
    pub uncaught_exception: Option<SharedItem>,

    /// The current state of the VM.
    pub state: VMState,

    pub is_jumping: bool,
}

impl ExecutionEngine {
    /// Constructs a new VM engine with default options.
    pub fn new() -> Self {
        Self::with_options(ExecutionEngineLimits::default())
    }

    /// Constructs a VM engine with the given options.
    pub fn with_options(limits: ExecutionEngineLimits) -> Self {
        Self {
            limits,
            reference_counter: Rc::new(RefCell::new(References::new())),
            invocation_stack: Vec::new(),
            current_context: None,
            entry_context: None,
            result_stack: Rc::new(RefCell::new(EvaluationStack::new(Rc::new(RefCell::new(
                References::new(),
            ))))),
            uncaught_exception: None,
            state: VMState::Break,
            is_jumping: false,
        }
    }

    /// Starts executing the loaded script.
    pub fn execute(&mut self) -> VMState {
        if self.state == VMState::Break {
            self.state = VMState::None;
        }

        while self.state != VMState::Halt && self.state != VMState::Fault {
            self.execute_next();
        }

        self.state
    }

    /// Steps through executing a single instr.
    ///
    fn execute_next(&mut self) {
        if self.invocation_stack.is_empty() {
            self.state = VMState::Halt;
        } else {
            let context = self.current_context?.borrow();

            let instruction = context.current_instruction().unwrap_or(Instruction::RET);

            self.pre_execute_instruction(instruction);

            match self.execute_instruction(instruction) {
                Ok(_) => (),
                Err(e) => Err(VMError::InvalidOpcode("{e}".parse().unwrap())), // self.on_fault(e),
            }

            &self.post_execute_instruction(instruction);
            if !self.is_jumping {
                self.current_context.unwrap().move_next();
            }

            self.is_jumping = false;
        }
    }

    fn pop(&mut self) -> &StackItem {
        let item = self
            .borrow()
            .current_context
            .unwrap()
            .get_mut()
            .evaluation_stack()
            .get_mut()
            .pop()
            .borrow();
        item.clone()
    }

    fn push(&mut self, item: SharedItem) {
        self.current_context.unwrap().get_mut().evaluation_stack().get_mut().push(item);
    }

    fn peek(&self, index: usize) -> SharedItem {
        let a = &self
            .clone()
            .current_context
            .unwrap()
            .borrow()
            .evaluation_stack()
            .get_mut()
            .peek(index as i32);
        a.clone()
    }

    fn execute_call(&mut self, offset: i32) {
        let new_context = self.current_context.unwrap().clone_at_offset(offset);
        self.load_context(new_context);
    }

    fn execute_jump_offset(&mut self, offset: i32) {
        self.execute_jump(
            (self.current_context?.borrow().instruction_pointer as i32)
                .checked_add(offset)
                .unwrap(),
        )
    }
    fn execute_jump(&mut self, offset: i32) {
        let new_ip = (self.current_context?.borrow().instruction_pointer as i32 + offset) as usize;
        if new_ip >= self.current_context?.borrow().script.0.len() {
            return self.handle_error(Error::InvalidJump);
        }
        self.current_context?.borrow().instruction_pointer = new_ip;
    }

    fn handle_error(&mut self, err: Error) {
        self.state = VMState::Fault;
        self.uncaught_exception = Some(StackItem::from(Null::default()).into());
    }

    fn load_context(&mut self, context: &Rc<RefCell<ExecutionContext>>) {
        self.invocation_stack.push(context.clone());
        self.current_context = Some(self.invocation_stack.last().unwrap().clone());
        if self.entry_context.is_none() {
            self.entry_context = self.current_context.clone();
        }
    }

    fn unload_context(&mut self, mut context: Rc<RefCell<ExecutionContext>>) {
        if self.invocation_stack.is_empty() {
            self.current_context = None;
            self.entry_context = None;
        } else {
            self.current_context = Some(self.invocation_stack.last().unwrap().clone());
        }

        if let Some(current) = &mut self.current_context {
            if current.borrow().fields() != context.borrow().fields() {
                context.borrow().fields()?.clear_references();
            }
        }
        context.borrow().local_variables.unwrap().clear_references();
        context.borrow().arguments.unwrap().clear_references();
    }

    fn create_context(
        &self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> ExecutionContext {
        let share = SharedStates {
            script,
            evaluation_stack: Default::default(),
            static_fields: None,
            states: Default::default(),
        };

        ExecutionContext {
            shared_states,
            instruction_pointer: initial_position,
            rv_count: rvcount,
            local_variables: None,
            try_stack: None,
            arguments: None,
        }
    }

    fn load_script(
        &mut self,
        script: Script,
        rvcount: i32,
        initial_position: usize,
    ) -> Rc<RefCell<ExecutionContext>> {
        let context = Rc::new(RefCell::new(self.create_context(script, rvcount, initial_position)));

        self.load_context(&context);

        context
    }

    fn pre_execute_instruction(&mut self, instruction: Instruction) {
        if self.reference_counter.borrow().count() > self.limits.max_stack_size {
            panic!("Max stack size exceeded");
        }

        match instruction {
            Instruction::JMP(offset) => {
                self.is_jumping = true;
            }
            Instruction::CALL(context) => {
                self.load_context(context);
            }
            _ => (),
        }
    }

    fn post_execute_instruction(&mut self, instruction: Instruction) {
        let count = self.reference_counter.borrow().references();
        if count > self.limits.max_stack_size {
            panic!("Max stack size exceeded: {}", count);
        }

        match instruction {
            Instruction::RET => {
                let context = self.invocation_stack.pop().unwrap();
                // do something with returned context
            }
            Instruction::THROW => {
                self.handle_exception();
            }
            _ => (),
        }
    }
    fn handle_exception(&mut self) {
        // loop through contexts
        // set instruction pointer to catch or finally
        // pop contexts
        if let Some(exception) = self.uncaught_exception.take() {
            panic!("Unhandled exception: {:?}", exception.borrow());
        }
    }

    fn execute_try(&mut self, catch_offset: usize, finally_offset: usize) {
        let context = self.current_context.as_mut().unwrap().borrow_mut();

        if catch_offset == 0 && finally_offset == 0 {
            panic!("Invalid try block offsets");
        }

        if context.try_stack.is_none() {
            context.try_stack = Some(Vec::new());
        }

        if context.try_stack.as_ref().unwrap().len() >= self.limits.max_try_nesting_depth {
            panic!("Max try nesting depth exceeded");
        }

        let catch_pointer = if catch_offset > 0 {
            Some(context.instruction_pointer + catch_offset)
        } else {
            None
        };

        let finally_pointer = if finally_offset > 0 {
            Some(context.instruction_pointer + finally_offset)
        } else {
            None
        };

        context.try_stack.as_mut().unwrap().push(ExceptionHandlingContext {
            state:           ExceptionHandlingState::Try,
            catch_pointer:   catch_pointer.unwrap() as i32,
            finally_pointer: finally_pointer.unwrap() as i32,
            end_pointer:     0,
        });

        self.is_jumping = true;
    }

    fn execute_throw(&mut self, exception: SharedItem) {
        self.uncaught_exception = Some(exception);
        self.handle_exception();
    }

    fn execute_end_try(&mut self, end_offset: usize) {
        let context = self.current_context.as_mut().unwrap().borrow_mut();

        let mut current_try = match context.try_stack.as_mut().unwrap().pop() {
            Some(try_context) => try_context,
            None => panic!("No matching try block found"),
        };

        if let ExceptionHandlingState::Finally = current_try.state() {
            panic!("EndTry cannot be called in finally block");
        }

        let end_pointer = context.instruction_pointer + end_offset;

        if let Some(handler) = current_try.finally_pointer() {
            current_try.set_state(ExceptionHandlingState::Finally);
            current_try.set_end_pointer(end_pointer as i32);
            context.instruction_pointer = handler;
        } else {
            context.instruction_pointer = end_pointer;
        }

        self.is_jumping = true;
    }

    fn execute_load_from_slot(&mut self, slot: &mut Slots, index: usize) {
        if let Some(values) = slot {
            if index < values.len() {
                let value = values[index].clone();
                self.push(value);
            } else {
                panic!("Invalid slot index: {}", index);
            }
        } else {
            panic!("Slot not initialized");
        }
    }

    fn execute_store_to_slot(&mut self, slot: &mut Slot, index: usize) {
        if let Some(slot) = slot {
            if index >= slot.len() {
                panic!("Index out of range when storing to slot: {}", index);
            }

            let value = self.result_stack.get_mut().pop();
            slot[index] = value;
        } else {
            panic!("Slot has not been initialized.");
        }
    }

    fn load_token(&mut self, token: u16) -> Result<ExecutionContext, &'static str> {
        panic!("Not implemented");
    }

    fn on_syscall(&mut self, method: u32) {
        panic!("Not implemented")
        // let syscall = match method {
        //     0 => Syscall::Syscall0,
        //     1 => Syscall::Syscall1,
        //     _ => panic!("Invalid syscall: {}", method),
        // };
        //
        // syscall.invoke(self);
    }
}
