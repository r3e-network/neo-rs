#[derive(Debug, Clone)]
pub(in crate::interpreter) struct TryFrame {
    pub(in crate::interpreter) catch_ip: usize,
    pub(in crate::interpreter) finally_ip: usize,
    pub(in crate::interpreter) call_depth: usize,
    pub(in crate::interpreter) caught: bool,
    pub(in crate::interpreter) in_finally: bool,
    pub(in crate::interpreter) end_ip: usize,
}
