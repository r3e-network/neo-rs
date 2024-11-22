// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::*;

pub struct ExecutionContext {
    pub(crate) statics:   Option<Slots>,
    pub(crate) locals:    Option<Slots>,
    pub(crate) arguments: Option<Slots>,

    // stack should be declared after than statics, locals and arguments
    pub(crate) stack: EvaluationStack,

    pc:      usize,
    program: Rc<Program>,
}

impl ExecutionContext {
    pub fn new(stack: EvaluationStack, program: Rc<Program>) -> Self {
        ExecutionContext { stack, statics: None, locals: None, arguments: None, pc: 0, program }
    }

    pub fn execute(&mut self) -> Result<(), ExecError> {
        let program = self.program.clone();
        let ops = program.ops();
        while self.pc < ops.len() {
            let op = &ops[self.pc];
            let code = op.code.as_u8() as usize;
            let _ = EXECUTORS[code](self, op)?; // TODO
        }

        Ok(())
    }

    #[inline]
    pub fn references(&self) -> &References {
        self.stack.references()
    }

    #[inline]
    pub fn on_terminated(&mut self) {
        self.pc = self.program.ops().len()
    }

    #[inline]
    pub fn change_pc(&mut self, to: u32) -> bool {
        self.program.ops().binary_search_by(|x| x.ip.cmp(&to)).map(|x| self.pc = x).is_ok()
    }

    #[inline]
    pub fn move_pc(&mut self) {
        if self.pc < self.program.ops().len() {
            self.pc += 1;
        }
    }
}
