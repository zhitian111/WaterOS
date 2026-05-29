//! Robust futex 用户态布局占位（与 Linux `struct robust_list_head` 对齐的 IPC 层视图）。

/// 用户态 robust 链表头布局占位。
///
/// 字段含义与 Linux ABI 一致；具体偏移校验与遍历逻辑由后续实现补齐。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RobustListHead {
    /// 链表头指针（用户地址）。
    pub list : usize,
    /// 自 `list` 节点到内嵌 futex 字的偏移。
    pub futex_offset : isize,
    /// 待处理 list_op（用户地址）。
    pub list_op_pending : usize,
}

/// Linux `set_robust_list(2)` 在 riscv64 等 64 位平台上的常见头结构大小。
pub const ROBUST_LIST_HEAD_SIZE : usize = core::mem::size_of::<RobustListHead>();
