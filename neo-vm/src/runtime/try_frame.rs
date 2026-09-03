#[derive(Debug, Clone)]
pub(super) struct TryFrame {
    pub(super) catch_pc: i32,
    pub(super) finally_pc: i32,
    pub(super) caught: bool,
    pub(super) in_finally: bool,
    pub(super) end_pc: i32,
}
