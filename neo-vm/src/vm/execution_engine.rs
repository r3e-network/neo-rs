use std::{
	cell::{Ref, RefCell},
	convert::TryInto,
	fmt::Error,
	ops::Neg,
	rc::Rc,
};
use crate::vm::{EvaluationStack, ExecContext, ExecutionEngineLimits, Instruction, OpCode, VMError, VMState};
use crate::vm_types::reference_counter::ReferenceCounter;
use crate::vm_types::stack_item::StackItem;

/// Represents the VM used to execute the script.
#[derive(Clone)]
pub struct NeoVm {
	/// Restrictions on the VM.
	pub limits: ExecutionEngineLimits,

	/// Used for reference counting of objects in the VM.
	pub reference_counter: Rc<RefCell<ReferenceCounter>>,

	/// The invocation stack of the VM.
	pub invocation_stack: Vec<Rc<RefCell<ExecContext>>>,

	/// The top frame of the invocation stack.
	pub current_context: Option<Rc<RefCell<ExecContext>>>,

	/// The bottom frame of the invocation stack.
	pub entry_context: Option<Rc<RefCell<ExecContext>>>,

	/// The stack to store the return values.
	pub result_stack: Rc<RefCell<EvaluationStack>>,

	/// The VM object representing the uncaught exception.
	pub uncaught_exception: Option<Rc<RefCell< StackItem>>>,

	/// The current state of the VM.
	pub state: VMState,

	pub is_jumping: bool,
}

/// Interface implemented by objects that can be reference counted.
pub trait ReferenceCounted {
	/// Returns a unique ID for the object.
	fn id(&self) -> usize;

	/// Free any resources used by the object.
	fn free(&mut self);
}

impl<T> ReferenceCounted for T
where
	T: Sized + PartialEq,
{
	fn id(&self) -> usize {
		self as *const T as usize
	}

	fn free(&mut self) {}
}

impl NeoVm {
	/// Constructs a new VM engine with default options.
	pub fn new() -> Self {
		Self::with_options(ExecutionEngineLimits::default())
	}

	/// Constructs a VM engine with the given options.
	pub fn with_options(limits: ExecutionEngineLimits) -> Self {
		Self {
			limits,
			reference_counter: Rc::new(RefCell::new(ReferenceCounter::new())),
			invocation_stack: Vec::new(),
			current_context: None,
			entry_context: None,
			result_stack: Rc::new(RefCell::new(EvaluationStack::new(Rc::new(RefCell::new(
				ReferenceCounter::new(),
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
		let item = self.borrow().current_context
			.unwrap()
			.get_mut()
			.evaluation_stack()
			.get_mut()
			.pop().borrow();
		item.clone()
	}

	fn push(&mut self, item: Rc<RefCell< StackItem>>) {
		self.current_context
			.unwrap()
			.get_mut()
			.evaluation_stack()
			.get_mut()
			.push(item);
	}

	fn peek(&self, index: usize) -> Rc<RefCell< StackItem>> {
		let a = &self.clone().current_context
			.unwrap()
			.borrow()
			.evaluation_stack()
			.get_mut()
			.peek(index as i32);
		a.clone()
	}

	fn execute_instr(&mut self, instr: Instruction) -> Result<VMState, VMError> {
		match instr.opcode {
			//Push
			OpCode::PushInt8
			| OpCode::PushInt16
			| OpCode::PushInt32
			| OpCode::PushInt64
			| OpCode::PushInt128
			| OpCode::PushInt256 => self.push(Rc::new(RefCell::new(Integer::new(&BigInt::from_be_bytes(instr.operand.as_slice()))))),
			OpCode::PushTrue => self.push(Rc::new(RefCell::new(Boolean::new(true)))),
			OpCode::PushFalse => self.push(Rc::new(RefCell::new(Boolean::new(false)))),
			OpCode::PushA => {
				let position = (self.current_context?.get_mut().instruction_pointer as i32)
					.checked_add(instr.token_i32())
					.unwrap();
				if position < 0
					|| position > self.current_context?.get_mut().script().len() as i32
				{
					// return Err(VMException::InvalidOpcode("Bad pointer address: {position}");
					return Err(VMError::new(Error::new("Bad pointer address")))
				}

				self.push(
					StackItem::Pointer(Pointer::new(
						&self.current_context?.get_mut().script(),
						position as usize,
					))
					.into(),
				)
			},
			OpCode::PushNull => self.push(StackItem::Nu(Null::default()).into()),
			OpCode::PushData1 | OpCode::PushData2 | OpCode::PushData4 => {
				self.limits.assert_max_item_size(instr.operand.len() as u32);
				self.push(StackItemTrait::from(instr.operand).into())
			},
			OpCode::PushM1
			| OpCode::Push0
			| OpCode::Push1
			| OpCode::Push2
			| OpCode::Push3
			| OpCode::Push4
			| OpCode::Push5
			| OpCode::Push6
			| OpCode::Push7
			| OpCode::Push8
			| OpCode::Push9
			| OpCode::Push10
			| OpCode::Push11
			| OpCode::Push12
			| OpCode::Push13
			| OpCode::Push14
			| OpCode::Push15
			| OpCode::Push16 => self.push(StackItem::Integer(instr.opcode - OpCode::Push0).into()),

			// Control
			OpCode::Nop => Ok(VMState::None),
			OpCode::Jmp => self.execute_jump_offset(instr.token_i8() as i32),
			OpCode::JmpL => self.execute_jump_offset(instr.token_i32()),
			OpCode::JmpIf =>
				if self.pop().get_bool() {
					self.execute_jump_offset(instr.token_i8() as i32)
				},
			OpCode::JmpIfL =>
				if self.pop().get_bool() {
					self.execute_jump_offset(instr.token_i32())
				},
			OpCode::JmpIfNot =>
				if !self.pop().get_bool() {
					self.execute_jump_offset(instr.token_i8() as i32)
				},
			OpCode::JmpIfNotL =>
				if !self.pop().get_bool() {
					self.execute_jump_offset(instr.token_i32())
				},
			OpCode::JmpEq => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 == x2 {
					self.execute_jump_offset(instr.token_i8() as i32)
				}
			},
			OpCode::JmpEqL => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 == x2 {
					self.execute_jump_offset(instr.token_i32())
				}
			},
			OpCode::JmpNe => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 != x2 {
					self.execute_jump_offset(instr.token_i8() as i32)
				}
			},
			OpCode::JmpNeL => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 != x2 {
					self.execute_jump_offset(instr.token_i32())
				}
			},
			OpCode::JmpGt => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 > x2 {
					self.execute_jump_offset(instr.token_i8() as i32)
				}
			},
			OpCode::JmpGtL => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 > x2 {
					self.execute_jump_offset(instr.token_i32())
				}
			},
			OpCode::JmpGe => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 >= x2 {
					self.execute_jump_offset(instr.token_i8() as i32)
				}
			},
			OpCode::JmpGeL => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 >= x2 {
					self.execute_jump_offset(instr.token_i32())
				}
			},
			OpCode::JmpLt => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 < x2 {
					self.execute_jump_offset(instr.token_i8() as i32)
				}
			},
			OpCode::JmpLtL => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 < x2 {
					self.execute_jump_offset(instr.token_i32())
				}
			},
			OpCode::JmpLe => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 <= x2 {
					self.execute_jump_offset(instr.token_i8() as i32)
				}
			},
			OpCode::JmpLeL => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				if x1 <= x2 {
					self.execute_jump_offset(instr.token_i32())
				}
			},
			OpCode::Call => self.execute_call(
				(self.current_context?.get_mut().instruction_pointer + instr.token_i8()) as i32,
			),
			OpCode::CallL => self
				.execute_call((self.current_context?.get_mut().instruction_pointer + instr.token_i32()) as i32),
			OpCode::CallA => {
				let x: Pointer = self.pop().into();
				if x.script() != self.current_context?.get_mut().script() {
					return Err(VMError::InvalidOpcode(
						"Pointers can't be shared between scripts".parse().unwrap(),
					))
				}
				self.execute_call(x.position() as i32)
			},
			OpCode::CallT => self.load_token(instr.token_u16()),
			OpCode::Abort =>
				Err(VMError::InvalidOpcode("{OpCode::ABORT} is executed.".parse().unwrap())),
			OpCode::Assert => {
				let x = self.pop().get_bool();
				if !x {
					Err(VMError::InvalidOpcode(
						"{OpCode::ASSERT} is executed with false result.".parse().unwrap(),
					))
				}
				// break;
			},
			OpCode::Throw => self.execute_throw(self.pop()),
			OpCode::Try => {
				let catch_offset = instr.token_i8();
				let finally_offset = instr.token_i8_1();
				self.execute_try(catch_offset as usize, finally_offset as usize)
				// break;
			},
			OpCode::TryL => {
				let catch_offset = instr.token_i32();
				let finally_offset = instr.token_i32_1();
				self.execute_try(catch_offset as usize, finally_offset as usize)
			},
			OpCode::EndTry => {
				let end_offset = instr.token_i8();
				self.execute_end_try(end_offset as usize)
			},
			OpCode::EndTryL => {
				let end_offset = instr.token_i32();
				self.execute_end_try(end_offset as usize)
			},
			OpCode::EndFinally => {
				if self.current_context?.get_mut().try_stack.is_none() {
					return Err(VMError::InvalidOpcode(
						"The corresponding TRY block cannot be found.".parse().unwrap(),
					))
				}
				let current_try = match self.current_context?.get_mut().try_stack {
					Some(ref mut x) => x,
					None =>
						return Err(VMError::InvalidOpcode(
							"The corresponding TRY block cannot be found.".parse().unwrap(),
						)),
				};

				if self.uncaught_exception.is_none() {
					self.current_context?.get_mut().instruction_pointer = current_try.get_mut().EndPointer;
				} else {
					self.handle_exception();
				}

				self.is_jumping = true
			},
			OpCode::Ret => {
				let mut context_pop = self.invocation_stack.pop().unwrap();
				let stack_eval = match self.invocation_stack.len() == 0 {
					true => self.result_stack.clone(),
					false => self
						.invocation_stack
						.last()
						.unwrap()
						.borrow()
						.evaluation_stack()
						.clone(),
				};
				// }
				// ? self.result_stack.clone() : self.invocation_stack.self.peek().EvaluationStack;
				if context_pop.borrow().evaluation_stack() != stack_eval {
					if context_pop.borrow().rv_count >= 0
						&& context_pop.get_mut().evaluation_stack().get_mut().len() != context_pop.borrow().rv_count as usize
					{
						return Err(VMError::InvalidOpcode(
							"RVCount doesn't match with EvaluationStack".parse().unwrap(),
						))
					}
					context_pop.get_mut().evaluation_stack().CopyTo(stack_eval);
				}
				if self.invocation_stack.len() == 0 {
					self.state = VMState::Halt;
				}

				self.unload_context(context_pop);
				self.is_jumping = true
				// break;
			},
			OpCode::Syscall => self.on_syscall(instr.token_u32()),

			// Stack ops
			OpCode::Depth => self.push(self.current_context?.get_mut().evaluation_stack().borrow().len()),
			OpCode::Drop => self.pop(),
			OpCode::Nip => self.current_context.unwrap().evaluation_stack().remove(1),
			OpCode::Xdrop => {
				let n = self.pop().get_integer().to_i32().unwrap();
				if n < 0 {
					return Err(VMError::InvalidOpcode(
						"The negative value {n} is invalid for OpCode::{instr.OpCode}."
							.parse()
							.unwrap(),
					))
				}
				self.current_context.unwrap().evaluation_stack().remove(n as i64)
			},
			OpCode::Clear => self.current_context.unwrap().evaluation_stack().Clear(),
			OpCode::Dup => self.push(self.peek(0).clone()),
			OpCode::Over => self.push(self.peek(1).clone()),
			OpCode::Pick => {
				let n = self.pop().get_integer();
				if n < BigInt::zero() {
					return Err(VMError::InvalidOpcode(
						"The negative value {n} is invalid for OpCode::{instr.OpCode}."
							.parse()
							.unwrap(),
					))
				}
				self.push(self.peek(n.to_i32().unwrap() as usize).clone())
				// break;
			},
			OpCode::Tuck => self
				.current_context?
				.get_mut()
				.evaluation_stack()
				.get_mut()
				.Insert(2, self.peek(0)),
			OpCode::Swap => {
				let x = self.current_context.unwrap().evaluation_stack().remove(1);
				self.push(StackItemTrait::from(x).into())
				// break;
			},
			OpCode::Rot => {
				let x = self.current_context.unwrap().evaluation_stack().remove(2);
				self.push(StackItemTrait::from(x).into())
			},
			OpCode::Roll => {
				let n = self.pop().get_integer().to_i64().unwrap();
				if n < 0 {
					return Err(VMError::InvalidOpcode(
						"The negative value {n} is invalid for OpCode::{instr.OpCode}."
							.parse()
							.unwrap(),
					))
				}
				if n == 0 {
					return Ok(VMState::None)
				}
				let x = self.current_context?.get_mut().evaluation_stack().remove(n);
				self.push(StackItemTrait::from(x).into())
			},
			OpCode::Reverse3 => self.current_context?.get_mut().evaluation_stack().Reverse(3),
			OpCode::Reverse4 => self.current_context?.get_mut().evaluation_stack().Reverse(4),
			OpCode::ReverseN => {
				let n = self.pop().get_integer();
				self.current_context?.get_mut().evaluation_stack().Reverse(n)
			},

			//Slot
			OpCode::InitSSLot => {
				if self.current_context?.get_mut().static_fields().is_some() {
					return Err(VMError::InvalidOpcode(
						"{instr.OpCode} cannot be executed twice.".parse().unwrap(),
					))
				}
				if instr.token_u8() == 0 {
					return Err(VMError::InvalidOpcode(
						"The operand {instr.token_u8()} is invalid for OpCode::{instr.OpCode}."
							.parse()
							.unwrap(),
					))
				}
				self.current_context?.get_mut().set_fields() = Some(
					&Slot::new_with_count(instr.token_u8() as i32, self.reference_counter.clone()),
				)
				// break;
			},
			OpCode::InitSlot => {
				if self.current_context?.get_mut().local_variables.is_some()
					|| self.current_context?.get_mut().arguments.is_some()
				{
					return Err(VMError::InvalidOpcode(
						"{instr.OpCode} cannot be executed twice.".parse().unwrap(),
					))
				}
				if instr.token_u16() == 0 {
					return Err(VMError::InvalidOpcode(
						"The operand {instr.token_u16()} is invalid for OpCode::{instr.OpCode}."
							.parse()
							.unwrap(),
					))
				}
				if instr.token_u8() > 0 {
					self.current_context?.get_mut().local_variables = Some(Slot::new_with_count(
						instr.token_u8() as i32,
						self.reference_counter.clone(),
					));
				}
				if instr.token_u8_1() > 0 {
					// generate a vector of instr.token_u8_1() StackItems
					let mut items = Vec::new();
					let size = instr.token_u8_1() as usize;

					// for _ in 0..size{
					//     items.push(StackItem::default());
					// }
					//
					for i in 0..size {
						items[i] = self.pop();
					}

					self.current_context?.get_mut().arguments =
						Some(Slot::new(items, self.reference_counter.clone()))
				}
			},
			OpCode::LdSFLd0
			| OpCode::LdSFLd1
			| OpCode::LdSFLd2
			| OpCode::LdSFLd3
			| OpCode::LdSFLd4
			| OpCode::LdSFLd5
			| OpCode::LdSFLd6 => self.execute_load_from_slot(
				&mut self.current_context?.get_mut().fields().unwrap(),
				instr.opcode - OpCode::LdSFLd0,
			),
			OpCode::LdSFLd => self.execute_load_from_slot(
				&mut self.current_context?.get_mut().fields().unwrap(),
				instr.token_u8() as usize,
			),
			OpCode::StSFLd0
			| OpCode::StSFLd1
			| OpCode::StSFLd2
			| OpCode::StSFLd3
			| OpCode::StSFLd4
			| OpCode::StSFLd5
			| OpCode::StSFLd6 => self.execute_store_to_slot(
				&mut self.current_context?.get_mut().fields()?,
				instr.opcode - OpCode::StSFLd0,
			),
			OpCode::StSFLd => self.execute_store_to_slot(
				&mut self.current_context?.get_mut().fields()?,
				instr.token_u8() as usize,
			),
			OpCode::LdLoc0
			| OpCode::LdLoc1
			| OpCode::LdLoc2
			| OpCode::LdLoc3
			| OpCode::LdLoc4
			| OpCode::LdLoc5
			| OpCode::LdLoc6 => self.execute_load_from_slot(
				& mut self.current_context?.get_mut().local_variables?,
				instr.opcode - OpCode::LdLoc0,
			),
			OpCode::LdLoc => self.execute_load_from_slot(
				&mut self.current_context?.get_mut().local_variables?,
				instr.token_u8() as usize,
			),
			OpCode::StLoc0
			| OpCode::StLoc1
			| OpCode::StLoc2
			| OpCode::StLoc3
			| OpCode::StLoc4
			| OpCode::StLoc5
			| OpCode::StLoc6 => self.execute_store_to_slot(
				&mut self.current_context?.get_mut().local_variables?,
				instr.opcode - OpCode::StLoc0,
			),
			OpCode::StLoc => self.execute_store_to_slot(
				&mut self.current_context?.get_mut().local_variables?,
				instr.token_u8() as usize,
			),
			OpCode::LdArg0
			| OpCode::LdArg1
			| OpCode::LdArg2
			| OpCode::LdArg3
			| OpCode::LdArg4
			| OpCode::LdArg5
			| OpCode::LdArg6 => self.execute_load_from_slot(
				&mut self.current_context?.get_mut().arguments.unwrap(),
				instr.opcode - OpCode::LdArg0,
			),
			OpCode::LdArg => self.execute_load_from_slot(
				&mut self.current_context?.get_mut().arguments.unwrap(),
				instr.token_u8() as usize,
			),
			OpCode::StArg0
			| OpCode::StArg1
			| OpCode::StArg2
			| OpCode::StArg3
			| OpCode::StArg4
			| OpCode::StArg5
			| OpCode::StArg6 => self.execute_store_to_slot(
				&mut self.current_context?.get_mut().arguments?,
				instr.opcode - OpCode::StArg0,
			),
			OpCode::StArg => self.execute_store_to_slot(
				&mut self.current_context?.get_mut().arguments?,
				instr.token_u8() as usize,
			),

			// Splice
			OpCode::NewBuffer => {
				let length = self.pop().get_integer();
				self.limits.assert_max_item_size(length.to_u32().unwrap());
				self.push(StackItemTrait::from(Buffer::new(length.to_usize().unwrap())).into())
			},
			OpCode::MemCpy => {
				let count = self.pop().get_integer().to_i64().unwrap();
				if count < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let si = self.pop().get_integer().to_i64().unwrap();
				if si < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {si} is out of range.".parse().unwrap(),
					))
				}
				let src = self.pop().get_mut().get_slice();
				if si.checked_add(count).unwrap() > src.len() as i64 {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let di = self.pop().get_mut().get_integer().to_i64().unwrap();
				if di < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {di} is out of range.".parse().unwrap(),
					))
				}
				let dst: Buffer = self.pop().into();
				if di.checked_add(count)? > dst.size() as i64 {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				src.Slice(si, count).CopyTo(dst.get_slice()[di..])
			},
			OpCode::Cat => {
				let x2 = self.pop().GetSpan();
				let x1 = self.pop().GetSpan();
				let length = x1.Length + x2.Length;
				self.limits.assert_max_item_size(length);
				let result = Buffer::new(length); //, false);
				x1.CopyTo(result.get_slice());
				x2.CopyTo(result.get_slice()[x1.Length..]);
				self.push(StackItemTrait::from(result).into())
				// break;
			},
			OpCode::Substr => {
				let count = self.pop().get_integer().to_usize().unwrap();
				if count < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let index = self.pop().get_integer().to_usize().unwrap();
				if index < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {index} is out of range.".parse().unwrap(),
					))
				}
				let x = self.pop().GetSpan();
				if index + count > x.Length {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let result = Buffer::new(count); //, false);
				x.Slice(index, count).CopyTo(result.get_slice());
				self.push(StackItemTrait::from(result).into())
			},
			OpCode::Left => {
				let count = self.pop().get_integer().to_i32().unwrap();
				if count < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let x = self.pop().GetSpan();
				if count > x.Length {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let result = Buffer::new(count as usize); //, false);
				x[..count].CopyTo(result.get_slice());
				self.push(StackItemTrait::from(result).into())
			},
			OpCode::Right => {
				let count = self.pop().get_integer().to_i32().unwrap();
				if count < 0 {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let x = self.pop().get_slice();
				if count > x.Length {
					return Err(VMError::InvalidOpcode(
						"The value {count} is out of range.".parse().unwrap(),
					))
				}
				let result = Buffer::from(x); //, false);
							  // x[^count.. ^ 0].CopyTo(result.InnerBuffer.Span);
				self.push(StackItemTrait::from(result).into())
				// break;
			},

			// Bitwise logic
			OpCode::Invert => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(BigInt::neg(x)).into())
			},
			OpCode::And => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 & x2).into())
			},
			OpCode::Or => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 | x2).into())
			},
			OpCode::Xor => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 ^ x2).into())
			},
			OpCode::Equal => {
				let x2 = self.pop();
				let x1 = self.pop();
				self.push(x1.Equal(x2, self.limits))
			},
			OpCode::NotEqual => {
				let x2 = self.pop();
				let x1 = self.pop();
				self.push(!x1.Equals(x2, self.limits))
			},

			// Numeric
			OpCode::Sign => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(x.Sign).into())
			},
			OpCode::Abs => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(BigInt::abs(&x)).into())
			},
			OpCode::Negate => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(-x).into())
			},
			OpCode::Inc => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(x + 1).into())
			},
			OpCode::Dec => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(x - 1).into())
			},
			OpCode::Add => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 + x2).into())
			},
			OpCode::Sub => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 - x2).into())
			},
			OpCode::Mul => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 * x2).into())
			},
			OpCode::Div => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 / x2).into())
			},
			OpCode::Mod => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 % x2).into())
			},
			OpCode::Pow => {
				let exponent = self.pop().get_integer().to_i32().unwrap();
				self.limits.assert_shift(exponent);
				let value = self.pop().get_integer();
				self.push(StackItemTrait::from(value.pow(exponent as u32)).into())
			},
			OpCode::Sqrt => self.push(self.pop().get_integer().Sqrt()),
			OpCode::ModMul => {
				let modulus = self.pop().get_integer();
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 * x2 % modulus).into())
			},
			OpCode::ModPow => {
				let modulus = self.pop().get_integer();
				let exponent = self.pop().get_integer().to_i32().unwrap();
				let value = self.pop().get_integer();
				let result = match exponent == -1 {
					true => value.ModInverse(modulus),
					false => value.ModPow(exponent, modulus),
				};
				// } value.ModInverse(modulus) :  BigInteger.ModPow(value, exponent, modulus);
				self.push(StackItemTrait::from(result).into())
			},
			OpCode::Shl => {
				let shift = self.pop().get_integer().to_i32().unwrap();
				self.limits.assert_shift(shift);
				if shift == 0 {
					return Ok(VMState::None)
				}
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(x << shift).into())
			},
			OpCode::Shr => {
				let shift = self.pop().get_integer().to_i32().unwrap();
				self.limits.assert_shift(shift);
				if shift == 0 {
					return Ok(VMState::None) // break;
				}
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(x >> shift).into())
			},
			OpCode::Not => {
				let x = self.pop().get_bool();
				self.push(StackItemTrait::from(!x).into())
			},
			OpCode::BoolAnd => {
				let x2 = self.pop().get_bool();
				let x1 = self.pop().get_bool();
				self.push(StackItemTrait::from(x1 && x2).into())
			},
			OpCode::BoolOr => {
				let x2 = self.pop().get_bool();
				let x1 = self.pop().get_bool();
				self.push(StackItemTrait::from(x1 || x2).into())
			},
			OpCode::Nz => {
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(!x.is_zero()).into())
			},
			OpCode::NumEqual => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 == x2).into())
			},
			OpCode::NumNotEqual => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(x1 != x2).into())
			},
			OpCode::Lt => {
				let x2 = self.pop();
				let x1 = self.pop();
				if x1.get_item_type() == StackItemType::Any
					|| x2.get_item_type() == StackItemType::Any
				{
					self.push(StackItemTrait::from(false).into())
				} else {
					self.push(StackItemTrait::from(x1.get_integer() < x2.get_integer()).into())
				}
			},
			OpCode::Le => {
				let x2 = self.pop().borrow();
				let x1 = self.pop().borrow();
				if x1.get_type() == StackItemType::Any
					|| x2.get_item_type() == StackItemType::Any
				{
					self.push(StackItemTrait::from(false).into())
				} else {
					self.push(StackItemTrait::from(x1.get_integer() <= x2.get_integer()).into())
				}
				// break;
			},
			OpCode::Gt => {
				let x2 = self.pop().borrow();
				let x1 = self.pop().borrow();
				if x1.get_type() == StackItemType::Any
					|| x2.get_item_type() == StackItemType::Any
				{
					self.push(StackItemTrait::from(false).into())
				} else {
					self.push(StackItemTrait::from(x1.get_integer() > x2.get_integer()).into())
				}
				// break;
			},
			OpCode::Ge => {
				let x2 = self.pop();
				let x1 = self.pop();
				if x1.get_item_type() == StackItemType::Any
					|| x2.get_item_type() == StackItemType::Any
				{
					self.push(StackItemTrait::from(false).into())
				} else {
					self.push(StackItemTrait::from(x1.get_integer() >= x2.get_integer()).into())
				}
				// break;
			},
			OpCode::Min => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(BigInt::min(x1, x2)).into())
			},
			OpCode::Max => {
				let x2 = self.pop().get_integer();
				let x1 = self.pop().get_integer();
				self.push(StackItemTrait::from(BigInt::max(x1, x2)).into())
				// break;
			},
			OpCode::Within => {
				let b = self.pop().get_integer();
				let a = self.pop().get_integer();
				let x = self.pop().get_integer();
				self.push(StackItemTrait::from(a <= x && x < b).into())
			},

			// Compound-type
			OpCode::PackMap => {
				let size = self.pop().get_integer().to_usize().unwrap();
				if size < 0 || size * 2 > self.current_context?.get_mut().evaluation_stack().borrow().size() {
					return Err(VMError::InvalidOpcode(
						"The value {size} is out of range.".parse().unwrap(),
					))
				}
				let map = Map::new(Some(self.reference_counter.clone()));
				for i in 0..size {
					let key: dyn PrimitiveTrait = self.pop().into();
					let value = self.pop();
					map[key] = value;
				}
				self.push(StackItem::Map(map).into())
			},
			OpCode::PackStruct => {
				let size = self.pop().get_integer().to_i64().unwrap();
				if size < 0 || size > self.current_context?.get_mut().evaluation_stack().borrow().size() {
					return Err(VMError::InvalidOpcode(
						"The value {size} is out of range.".parse().unwrap(),
					))
				}
				let _struct = Struct::new(None, Some(self.reference_counter.clone()));
				for i in 0..size {
					let item = self.pop();
					_struct.Add(item);
				}
				self.push(StackItem::Struct(_struct).into())
				// break;
			},
			OpCode::Pack => {
				let size = self.pop().get_integer().to_usize().unwrap();
				if size < 0
					|| size > self.current_context.unwrap().evaluation_stack().len()
				{
					return Err(VMError::InvalidOpcode(
						"The value {size} is out of range.".parse().unwrap(),
					))
				}
				let array = Array::new(None, Some(self.reference_counter.clone()));
				for i in 0..size {
					let item = self.pop();
					array.Add(item);
				}
				self.push(StackItemTrait::from(array).into())
			},
			OpCode::Unpack => {
				let compound: dyn CompoundTrait = self.pop().into();
				match compound {
					CompoundTrait::VMMap(map) =>
						for (key, value) in map.values().rev() {
							self.push((value).into().into());
							self.push((key).into());
						},

					// break;
					CompoundTrait::VMArray(array) =>
						for i in (0..=array.array.len()).rev() {
							self.push(array[i].into());
						},
					// break;
					_ =>
						return Err(VMError::InvalidOpcode(
							"Invalid type for {instr.OpCode}: {compound.Type}".parse().unwrap(),
						)),
				}
				self.push(StackItemTrait::from(compound.count()).into())
			},
			OpCode::NewArray0 => self.push(
				StackItemTrait::from(Array::new(None, Some(self.reference_counter.clone()))).into(),
			),
			OpCode::NewArray | OpCode::NewArrayT => {
				let n = self.pop().get_integer().to_i64().unwrap();
				if n < 0 || n > self.limits.max_stack_size {
					return Err(VMError::InvalidOpcode(
						"MaxStackSize exceed: {n}".parse().unwrap(),
					))
				}
				let item:  StackItem;
				if instr.opcode == OpCode::NewArrayT {
					let _type = instr.token_u8();
					if !StackItemType::is_valid(_type) {
						return Err(VMError::InvalidOpcode(
							"Invalid type for {instr.OpCode}: {instr.token_u8()}".parse().unwrap(),
						))
					}
					item = match _type as StackItemType {
						StackItemType::Boolean => StackItemTrait::from(false),
						StackItemType::Integer => StackItemTrait::from(BigInt::zero()),
						StackItemType::ByteString => StackItemTrait::from(ByteString::new(Vec::new())),
						_ => StackItemTrait::from(Null::default()),
					};
				} else {
					item = StackItem::Null(Null::default());
				}
				self.push(
					StackItemTrait::from(Array::new(
						std::iter::repeat(item).take(n as usize).collect(),
						Some(self.reference_counter.clone()),
					))
					.into(),
				)
			},
			OpCode::NewStruct0 => self.push(
				StackItemTrait::from(Struct::new(None, Some(self.reference_counter.clone()))).into(),
			),
			OpCode::NewStruct => {
				let n = self.pop().get_integer() as usize;
				if n < 0 || n > self.limits.max_stack_size {
					return Err(VMError::InvalidOpcode(
						"MaxStackSize exceed: {n}".parse().unwrap(),
					))
				}
				let result = Struct::new(None, Some(self.reference_counter.clone()));
				for i in 0..n {
					result.Add(StackItemTrait::from(Null::default()));
				}
				self.push(StackItemTrait::from(result).into())
				// break;
			},
			OpCode::NewMap =>
				self.push(StackItem::from(Map::new(Some(self.reference_counter.clone()))).into()),
			OpCode::Size => {
				let x = self.pop();
				match x {
					StackItem::VMArray(array) => self.push(StackItemTrait::from(array.Count).into()),
					StackItem::VMMap(map) => self.push(StackItemTrait::from(map.Count).into()),
					StackItem::VMStruct(_struct) =>
						self.push(StackItemTrait::from(_struct.Count).into()),
					StackItem::VMByteString(array) => self.push(StackItemTrait::from(array.Size).into()),
					StackItem::VMBuffer(buffer) => self.push(StackItemTrait::from(buffer.Size).into()),
					StackItem::VMInteger(integer) =>
						self.push(StackItemTrait::from(integer.size()).into()),
					_ =>
						return Err(VMError::InvalidOpcode(
							"Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap(),
						)),
				}
			},
			OpCode::HasKey => {
				let key: Rc<RefCell<dyn PrimitiveTrait>> = self.pop().into();
				let x = self.pop();
				match x {
					StackItem::Map(map) =>
						self.push(StackItemTrait::from(map.contains_key(key)).into()),
					StackItem::ByteString(array) => {
						let index = key.get_integer().to_u32().unwrap();
						if index < 0 {
							return Err(VMError::InvalidOpcode(
								"The negative value {index} is invalid for OpCode::{instr.OpCode}."
									.parse()
									.unwrap(),
							))
						}
						self.push(StackItemTrait::from(index < array.size() as u32).into())
					},
					StackItem::Buffer(buffer) => {
						let index = key.get_integer().to_u32().unwrap();
						if index < 0 {
							return Err(VMError::InvalidOpcode(
								"The negative value {index} is invalid for OpCode::{instr.OpCode}."
									.parse()
									.unwrap(),
							))
						}
						self.push(StackItemTrait::from(index < buffer.size() as u32).into())
					},
					StackItem::Array(array) => {
						let index = key.get_integer().to_u32().unwrap();
						if index < 0 {
							return Err(VMError::InvalidOpcode(
								"The negative value {index} is invalid for OpCode::{instr.OpCode}."
									.parse()
									.unwrap(),
							))
						}

						self.push(StackItemTrait::from(index < array.count() as u32).into())
					},
					_ =>
						return Err(VMError::InvalidOpcode(
							"Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap(),
						)),
				}
				// break;
			},
			OpCode::Keys => {
				let map: Map = self.pop().into();
				self.push(
					StackItemTrait::from(Array::new(
						Some(map.keys()),
						Some(self.reference_counter.clone()),
					))
					.into(),
				)
			},
			OpCode::Values => {
				let x = self.pop();
				let values = match x {
					StackItem::Array(array) => array,
					StackItem::Map(map) => map.values(),
					_ => panic!(), //return Err(VMException::InvalidOpcode("Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap())),
				};
				let mut new_array = Array::new(None, Some(self.reference_counter.clone()));
				for item in values.array {
					if item.get_item_type() == StackItemType::Struct {
						let s: Struct = item.into();
						new_array.add(s.clone(&self.limits).try_into().unwrap());

					// new_array.Add(s.Clone(self.limits));
					} else {
						new_array.Add(item);
					}
				}

				self.push(StackItemTrait::from(new_array).into())
			},
			OpCode::PickItem => {
				let key: Rc<RefCell<dyn PrimitiveTrait>> = self.pop().into();
				let x = self.pop();
				match x {
					StackItem::Array(array) => {
						let index = key.get_integer().to_i64().unwrap();
						if index < 0 || index >= array.Count {
							return Err(VMError::InvalidOpcode(
								"The value {index} is out of range.".parse().unwrap(),
							))
						}
						self.push(array[index])
					},
					StackItem::Map(map) => {
						let value = match map.get(key) {
							Some(v) => v,
							None =>
								return Err(VMError::InvalidOpcode(
									"Key not found in {nameof(Map)}".parse().unwrap(),
								)),
						};
						self.push(StackItemTrait::from(value).into())
					},
					StackItem::ByteString(byte_string) => {},
					StackItem::Boolean(boolean) => {},
					StackItem::Integer(integer) => {
						let byte_array = integer.get_slice();
						let index = key.get_integer().to_i64().unwrap();
						if index < 0 || index >= byte_array.Length {
							return Err(VMError::InvalidOpcode(
								"The value {index} is out of range.".parse().unwrap(),
							))
						}
						self.push(
							StackItemTrait::from(BigInt::from_bytes_le(
								Sign::NoSign,
								byte_array.get(index).unwrap(),
							))
							.into(),
						)
					},
					StackItem::Buffer(buffer) => {
						let index = key.get_integer().to_i64().unwrap();
						if index < 0 || index >= buffer.Size {
							return Err(VMError::InvalidOpcode(
								"The value {index} is out of range.".parse().unwrap(),
							))
						}
						self.push(
							StackItemTrait::from(BigInt::from_bytes_le(
								Sign::NoSign,
								buffer.get_slice().get(index).unwrap(),
							))
							.into(),
						)
					},
					_ =>
						return Err(VMError::InvalidOpcode(
							"Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap(),
						)),
				}
				// break;
			},
			OpCode::Append => {
				let mut new_item = self.pop();
				let array: Array = self.pop().into();
				if new_item.get_item_type() == StackItemType::Struct {
					let s: Struct = new_item.into();
					new_item = s.clone(&self.limits).try_into().unwrap();
					// new_item = s.Clone(self.limits);
				}
				array.Add(new_item)
			},
			OpCode::SetItem => {
				let mut value = self.pop();
				if value.get_item_type() == StackItemType::Struct {
					let s: Struct = value.into();
					value = s.clone(&self.limits).try_into().unwrap();
				}
				let key: dyn PrimitiveTrait = self.pop().into();
				let x = self.pop();
				match x {
					VMArray(array) => {
						let index = key.get_integer().to_i32().unwrap();
						if index < 0 || index >= array.Count {
							return Err(VMError::InvalidOpcode(
								"The value {index} is out of range.".parse().unwrap(),
							))
						}
						array[index] = value
					},
					StackItem::Map(map) => map[key] = value,
					StackItem::Buffer(buffer) => {
						let index = key.get_integer().to_i32().unwrap();
						if index < 0 || index >= buffer.Size {
							return Err(VMError::InvalidOpcode(
								"The value {index} is out of range.".parse().unwrap(),
							))
						}
						if !StackItemType::is_primitive(value.get_item_type() as u8) {
							return Err(VMError::InvalidOpcode(
								"Value must be a primitive type in {instr.OpCode}".parse().unwrap(),
							))
						}
						let b = value.get_integer().to_i64().unwrap();
						if b < i8::min as i64 || b > i8::max as i64 {
							return Err(VMError::InvalidOpcode(
								"Overflow in {instr.OpCode}, {b} is not a byte type."
									.parse()
									.unwrap(),
							))
						}
						buffer.InnerBuffer.Span[index] = b
					},
					_ => Err(VMError::InvalidOpcode(
						"Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap(),
					)),
				}
				// break;
			},
			OpCode::ReverseItems => {
				let x = self.pop();
				match x {
					StackItem::Array(array) => array.Reverse(),
					StackItem::Buffer(buffer) => buffer.InnerBuffer.Span.Reverse(),
					_ => Err(VMError::InvalidOpcode(
						"Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap(),
					)),
				}
			},
			OpCode::Remove => {
				let key: Rc<RefCell<dyn PrimitiveTrait>> = self.pop().into();
				let x = self.pop();
				match x {
					StackItem::Array(mut array) => {
						let index = key.get_integer().to_i32().unwrap();
						if index < 0 || index >= array.Count {
							return Err(VMError::InvalidOpcode(
								"The value {index} is out of range.".parse().unwrap(),
							))
						}
						array.remove_at(index as usize)
					},
					StackItem::Map(mut map) => map.remove(key),
					_ =>
						return Err(VMError::InvalidOpcode(
							"Invalid type for {instr.OpCode}: {x.Type}".parse().unwrap(),
						)),
				}
			},
			OpCode::ClearItems => {
				let x: dyn CompoundTrait = self.pop().into();
				x.Clear()
			},
			OpCode::PopItem => {
				let mut x: Rc<RefCell<dyn CompoundTrait>> = self.pop();
				let index = x.count() - 1;
				self.push(x[index].clone());
				x.remove_at(index)
			},

			//Types
			OpCode::IsNull => {
				let x = self.pop();
				self.push(StackItemTrait::from(x.get_item_type() == StackItemType::Any).into())
			},
			OpCode::IsType => {
				let x = self.pop();
				let _type: StackItemType = instr.token_u8() as StackItemType;
				if _type == StackItemType::Any || !StackItemType::is_valid(instr.token_u8()) {
					return Err(VMError::InvalidOpcode("Invalid type: {type}".parse().unwrap()))
				}
				self.push(StackItemTrait::from(x.get_item_type() == _type).into())
			},
			OpCode::Convert => {
				let x = self.pop();
				self.push(x.ConvertTo(instr.token_u8()))
			},
			OpCode::AbortMsg => {
				let msg = self.pop().GetString();
				Err(VMError::InvalidOpcode(
					"{OpCode::ABORTMSG} is executed. Reason: {msg}".parse().unwrap(),
				))
			},
			OpCode::AssertMsg => {
				let msg = self.pop().GetString();
				let x = self.pop().get_bool();
				if !x {
					return Err(VMError::InvalidOpcode(
						"{OpCode::ASSERTMSG} is executed with false result. Reason: {msg}"
							.parse()
							.unwrap(),
					))
				}
				// break;
			},
			_ => panic!("Opcode {instr} is undefined."),
		}

		Ok(VMState::Halt)
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
			return self.handle_error(Error::InvalidJump)
		}
		self.current_context?.borrow().instruction_pointer = new_ip;
	}

	fn handle_error(&mut self, err: Error) {
		self.state = VMState::Fault;
		self.uncaught_exception = Some(StackItemTrait::from(Null::default()).into());
	}

	fn load_context(&mut self, context: &Rc<RefCell<ExecContext>>) {
		self.invocation_stack.push(context.clone());
		self.current_context = Some(self.invocation_stack.last().unwrap().clone());
		if self.entry_context.is_none() {
			self.entry_context = self.current_context.clone();
		}
	}

	fn unload_context(&mut self, mut context: Rc<RefCell<ExecContext>>) {
		if self.invocation_stack.is_empty() {
			self.current_context = None;
			self.entry_context = None;
		} else {
			self.current_context = Some(self.invocation_stack.last().unwrap().clone());
		}

		if let Some(current) = &mut self.current_context {
			if current.borrow().fields()
				!= context.borrow().fields()
			{
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
	) -> ExecContext {
		let share = SharedStates {
			script,
			evaluation_stack: Default::default(),
			static_fields: None,
			states: Default::default(),
		};

		ExecContext {
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
	) -> Rc<RefCell<ExecContext>> {
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
			},
			Instruction::CALL(context) => {
				self.load_context(context);
			},
			_ => (),
		}
	}

	fn post_execute_instruction(&mut self, instruction: Instruction) {
		let count = self.reference_counter.borrow().count();
		if count > self.limits.max_stack_size {
			panic!("Max stack size exceeded: {}", count);
		}

		match instruction {
			Instruction::RET => {
				let context = self.invocation_stack.pop().unwrap();
				// do something with returned context
			},
			Instruction::THROW => {
				self.handle_exception();
			},
			_ => (),
		}
	}
	fn handle_exception(&mut self) {
		// loop through contexts
		// set instruction pointer to catch or finally
		// pop contexts
		if let Some(exception) = self.uncaught_exception.take() {
			panic!("Unhandled exception: {:?}", exception.borrow().get_slice());
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

		let catch_pointer =
			if catch_offset > 0 { Some(context.instruction_pointer + catch_offset) } else { None };

		let finally_pointer = if finally_offset > 0 {
			Some(context.instruction_pointer + finally_offset)
		} else {
			None
		};

		context.try_stack.as_mut().unwrap().push(ExceptionHandlingContext {
			state: ExceptionHandlingState::Try,
			catch_pointer: catch_pointer.unwrap() as i32,
			finally_pointer: finally_pointer.unwrap() as i32,
			end_pointer: 0,
		});

		self.is_jumping = true;
	}

	fn execute_throw(&mut self, exception: Rc<RefCell< StackItem>>) {
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

	fn execute_load_from_slot(&mut self, slot: &mut Slot, index: usize) {
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

	fn load_token(&mut self, token: u16) -> Result<ExecContext, &'static str> {
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
