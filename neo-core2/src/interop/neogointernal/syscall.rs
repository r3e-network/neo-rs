pub mod neogointernal {

    // Syscall0 performs syscall with 0 arguments.
    pub fn syscall0(name: &str) -> Option<Box<dyn std::any::Any>> {
        None
    }

    // Syscall0NoReturn performs syscall with 0 arguments.
    pub fn syscall0_no_return(name: &str) {
    }

    // Syscall1 performs syscall with 1 argument.
    pub fn syscall1(name: &str, arg: &dyn std::any::Any) -> Option<Box<dyn std::any::Any>> {
        None
    }

    // Syscall1NoReturn performs syscall with 1 argument.
    pub fn syscall1_no_return(name: &str, arg: &dyn std::any::Any) {
    }

    // Syscall2 performs syscall with 2 arguments.
    pub fn syscall2(name: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any) -> Option<Box<dyn std::any::Any>> {
        None
    }

    // Syscall2NoReturn performs syscall with 2 arguments.
    pub fn syscall2_no_return(name: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any) {
    }

    // Syscall3 performs syscall with 3 arguments.
    pub fn syscall3(name: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any, arg3: &dyn std::any::Any) -> Option<Box<dyn std::any::Any>> {
        None
    }

    // Syscall3NoReturn performs syscall with 3 arguments.
    pub fn syscall3_no_return(name: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any, arg3: &dyn std::any::Any) {
    }

    // Syscall4 performs syscall with 4 arguments.
    pub fn syscall4(name: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any, arg3: &dyn std::any::Any, arg4: &dyn std::any::Any) -> Option<Box<dyn std::any::Any>> {
        None
    }

    // Syscall4NoReturn performs syscall with 4 arguments.
    pub fn syscall4_no_return(name: &str, arg1: &dyn std::any::Any, arg2: &dyn std::any::Any, arg3: &dyn std::any::Any, arg4: &dyn std::any::Any) {
    }
}
