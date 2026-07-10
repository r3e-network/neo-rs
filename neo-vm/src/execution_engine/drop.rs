//
// drop.rs - Drop implementation for ExecutionEngine
//

use super::ExecutionEngine;

impl<S> Drop for ExecutionEngine<S> {
    fn drop(&mut self) {
        // Clear host references to avoid dangling pointers
        self.interop_host = None;
        // Clear the invocation stack
        self.invocation_stack.clear();
    }
}
