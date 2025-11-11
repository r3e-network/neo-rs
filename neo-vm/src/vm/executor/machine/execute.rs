use crate::{error::VmError, instruction::Instruction, value::VmValue};

use super::VirtualMachine;

impl<'a> VirtualMachine<'a> {
    pub(super) fn run(mut self) -> Result<VmValue, VmError> {
        loop {
            let instruction = self
                .instructions
                .get(self.ip)
                .cloned()
                .ok_or(VmError::Fault)?;
            self.ip += 1;
            if let Instruction::Return = instruction {
                break;
            }
            self.execute_instruction(instruction)?;
        }

        Ok(self.stack.pop().unwrap_or(VmValue::Null))
    }

    fn execute_instruction(&mut self, instruction: Instruction) -> Result<(), VmError> {
        match instruction {
            Instruction::PushInt(value) => {
                self.exec_push_int(value);
                Ok(())
            }
            Instruction::PushBool(value) => {
                self.exec_push_bool(value);
                Ok(())
            }
            Instruction::PushBytes(bytes) => {
                self.exec_push_bytes(bytes);
                Ok(())
            }
            Instruction::Add => self.exec_add(),
            Instruction::Sub => self.exec_sub(),
            Instruction::Mul => self.exec_mul(),
            Instruction::Div => self.exec_div(),
            Instruction::And => self.exec_and(),
            Instruction::Or => self.exec_or(),
            Instruction::Not => self.exec_not(),
            Instruction::Store(index) => self.exec_store(index),
            Instruction::Load(index) => self.exec_load(index),
            Instruction::Dup(depth) => self.exec_dup(depth),
            Instruction::Swap(depth) => self.exec_swap(depth),
            Instruction::Drop => self.exec_drop(),
            Instruction::Over => self.exec_over(),
            Instruction::Pick(depth) => self.exec_pick(depth),
            Instruction::Roll(depth) => self.exec_roll(depth),
            Instruction::Mod => self.exec_mod(),
            Instruction::Equal => self.exec_equal(),
            Instruction::Greater => self.exec_greater(),
            Instruction::Less => self.exec_less(),
            Instruction::GreaterOrEqual => self.exec_greater_or_equal(),
            Instruction::LessOrEqual => self.exec_less_or_equal(),
            Instruction::NotEqual => self.exec_not_equal(),
            Instruction::Xor => self.exec_xor(),
            Instruction::Shl => self.exec_shl(),
            Instruction::Shr => self.exec_shr(),
            Instruction::ToBool => self.exec_to_bool(),
            Instruction::ToInt => self.exec_to_int(),
            Instruction::ToBytes => self.exec_to_bytes(),
            Instruction::ToString => self.exec_to_string(),
            Instruction::Syscall(name) => self.exec_syscall(name),
            Instruction::Negate => self.exec_negate(),
            Instruction::Inc => self.exec_inc(),
            Instruction::Dec => self.exec_dec(),
            Instruction::Sign => self.exec_sign(),
            Instruction::Abs => self.exec_abs(),
            Instruction::Jump(target) => self.exec_jump(target),
            Instruction::JumpIfFalse(target) => self.exec_jump_if_false(target),
            Instruction::CallNative {
                contract,
                method,
                arg_count,
            } => self.exec_call_native(contract, method, arg_count),
            Instruction::Return => unreachable!("handled before dispatch"),
        }
    }
}
