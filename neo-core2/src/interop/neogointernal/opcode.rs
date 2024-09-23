pub mod neogointernal {

    // Opcode0NoReturn emits opcode without arguments.
    pub fn opcode0_no_return(op: &str) {
    }

    // Opcode1 emits opcode with 1 argument.
    pub fn opcode1(op: &str, arg: &dyn std::any::Any) -> Option<&dyn std::any::Any> {
        None
    }

    // Opcode1NoReturn emits opcode with 1 argument and no return value.
    pub fn opcode1_no_return(op: &str, arg: &dyn std::any::Any) {
    }

    // Opcode2 emits opcode with 2 arguments.
    pub fn opcode2(op: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any) -> Option<&dyn std::any::Any> {
        None
    }

    // Opcode2NoReturn emits opcode with 2 arguments and no return value.
    pub fn opcode2_no_return(op: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any) {
    }

    // Opcode3 emits opcode with 3 arguments.
    pub fn opcode3(op: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any, arg3: &dyn std::any::Any) -> Option<&dyn std::any::Any> {
        None
    }
}
