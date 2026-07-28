//! Robust futex 用户态布局（与 Linux `struct robust_list_head` 对齐的 IPC 层视图）。

/// Linux `FUTEX_OWNER_DIED` 标志。
pub const FUTEX_OWNER_DIED : u32 = 0x4000_0000;

/// futex 字中表示存在等待者的标志。
pub const FUTEX_WAITERS : u32 = 0x8000_0000;

/// futex 字中 TID 位掩码（低 30 位）。
pub const FUTEX_TID_MASK : u32 = 0x3FFF_FFFF;

/// 退出清理遍历 robust 链表的最大步数（防用户态坏链死循环）。
pub const ROBUST_LIST_LIMIT : usize = 4096;

/// 用户态 `struct robust_list` 中 `list` 指针字段大小（64-bit）。
pub const ROBUST_LIST_ENTRY_SIZE : usize = core::mem::size_of::<usize>();

/// 用户态 robust 链表头布局。
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

/// Linux `set_robust_list(2)` 在 riscv64 等 64 位平台上的头结构大小。
pub const ROBUST_LIST_HEAD_SIZE : usize = core::mem::size_of::<RobustListHead>();

/// 线程登记的 robust 链表及其所属用户地址空间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobustListRegistration {
    pub head : usize,
    pub len : usize,
    pub user_aspace : usize,
}
