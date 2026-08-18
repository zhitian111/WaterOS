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

/*
普通 futex 锁有个经典死锁 bug：线程 A 持有锁后崩溃/被杀/异常退出，锁永远处于"已锁定"状态，其他线程永久阻塞。

Robust futex 的解法：线程在用户态维护一条 robust 链表，记录"当前我持有的所有锁"。线程退出时内核遍历这条链表，把每个 futex 字标记成 FUTEX_OWNER_DIED（拥有者已死） 并唤醒等待者——等待者拿到锁后检测到 DIED 标志，就知道"原来的 owner 死了"，可以做清理而不是死等。
futex 字的编码：低 30 位是持有者线程的 TID；bit30，拥有者已死。；bit31，有线程在等这把锁。


*/

/// robust futex（健壮 futex） 的内核布局定义
///
/// ABI: 字段顺序和宽度必须与 64 位 Linux `struct robust_list_head` 保持一致；
/// IPC 层只描述布局，实际用户指针访问由 syscall 层完成。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RobustListHead {
    /// 链表头指针（指向第一个持锁节点）
    pub list : usize,
    /// 从链表节点到内嵌 futex 字的偏移
    pub futex_offset : isize,
    /// 正在进行的操作节点（供中途退出时定位）
    pub list_op_pending : usize,
}

/// Linux `set_robust_list(2)` 在 riscv64 等 64 位平台上的头结构大小。
pub const ROBUST_LIST_HEAD_SIZE : usize = core::mem::size_of::<RobustListHead>();

/// 线程登记的 robust 链表及其所属用户地址空间。
///
/// 生命周期：`set_robust_list` 写入、退出路径 `take_robust_list` 一次性取走，
/// 之后由 syscall 层完成 `FUTEX_OWNER_DIED` 清理与唤醒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RobustListRegistration {
    /// 用户地址空间中的 robust 链表头指针；必须按用户地址验证后才能解引用。
    pub head : usize,
    /// 头结构字节长度；当前 ABI 要求为 `ROBUST_LIST_HEAD_SIZE`。
    pub len : usize,
    /// 登记时所属地址空间身份，防止退出清理访问已切换地址空间。
    pub user_aspace : usize,
}
