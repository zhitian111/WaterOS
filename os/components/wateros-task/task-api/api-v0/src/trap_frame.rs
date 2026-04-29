/// 对外暴露的 trap 现场语义快照。
///
/// 完整 trap frame 的寄存器布局属于 `wateros-platform-arch` 的具体架构实现。
/// task API 只暴露上层通常需要观察的稳定语义，避免公共快照绑定到某个
/// 架构的寄存器数量、状态寄存器位布局或汇编保存顺序。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskTrapSnapshot {
    /// 原始 trap cause 编码，具体解释由当前架构约定。
    pub raw_cause: usize,
    /// trap 发生或恢复时关联的用户态 PC。
    pub user_pc: usize,
    /// trap 发生或恢复时关联的用户态 SP。
    pub user_sp: usize,
    /// fault 地址或架构提供的附加 trap 值。
    pub fault_addr: usize,
    /// 该现场恢复时是否会返回用户态。
    pub returns_to_user: bool,
}

impl TaskTrapSnapshot {
    /// 构造一份架构无关的 trap 语义快照。
    #[inline]
    pub const fn new(
        raw_cause: usize,
        user_pc: usize,
        user_sp: usize,
        fault_addr: usize,
        returns_to_user: bool,
    ) -> Self {
        Self {
            raw_cause,
            user_pc,
            user_sp,
            fault_addr,
            returns_to_user,
        }
    }

    /// 返回原始 trap cause 编码。
    #[inline]
    pub const fn raw_cause(&self) -> usize {
        self.raw_cause
    }

    /// 返回 trap 关联的用户态 PC。
    #[inline]
    pub const fn user_pc(&self) -> usize {
        self.user_pc
    }

    /// 返回 trap 关联的用户态 SP。
    #[inline]
    pub const fn user_sp(&self) -> usize {
        self.user_sp
    }

    /// 返回 fault 地址或架构提供的附加 trap 值。
    #[inline]
    pub const fn fault_addr(&self) -> usize {
        self.fault_addr
    }

    /// 判断该现场恢复时是否会返回用户态。
    #[inline]
    pub const fn returns_to_user(&self) -> bool {
        self.returns_to_user
    }

    /// 判断该现场恢复时是否会返回内核态。
    #[inline]
    pub const fn returns_to_kernel(&self) -> bool {
        !self.returns_to_user
    }
}
