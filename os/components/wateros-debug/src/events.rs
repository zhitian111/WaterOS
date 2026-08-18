//! GDB 诊断事件与锁类别的稳定编号。

/// 稳定的事件编号；只能追加，不能复用旧编号。
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugEventKind {
    /// 任务进入可运行队列。
    TaskEnqueue = 1,
    /// 发生上下文切换。
    TaskSwitch = 2,
    /// 任务进入阻塞状态。
    TaskBlock = 3,
    /// 阻塞任务被唤醒。
    TaskWake = 4,
    /// 任务退出。
    TaskExit = 5,
    /// 进入系统调用入口。
    SyscallEnter = 6,
    /// 离开系统调用入口。
    SyscallExit = 7,
    /// 进入异常/中断处理。
    TrapEnter = 8,
    /// 离开异常/中断处理。
    TrapExit = 9,
    /// 定时器 tick 到达。
    Timer = 10,
    /// 向其他 CPU 发送 IPI。
    IpiSend = 11,
    /// 收到 IPI。
    IpiReceive = 12,
    /// futex 开始等待。
    FutexWait = 13,
    /// futex 唤醒等待者。
    FutexWake = 14,
    /// 发起 TLB shootdown。
    TlbShootdown = 15,
    /// 等待锁时发生竞争。
    LockContended = 16,
    /// 成功获取锁。
    LockAcquire = 17,
    /// 释放锁。
    LockRelease = 18,
}

/// 主机报告中显示的关键锁类别。
#[repr(u16)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugLockKind {
    #[default]
    None = 0,
    /// 调度器锁。
    Scheduler = 1,
    /// 进程注册表锁。
    ProcessRegistry = 2,
    /// futex 注册表锁。
    FutexRegistry = 3,
    /// 物理帧分配器锁。
    FrameAllocator = 4,
    /// 地址空间锁。
    AddressSpace = 5,
    /// VFS 锁。
    Vfs = 6,
    /// 网络栈锁。
    Network = 7,
    /// 内核日志锁。
    Klog = 8,
}
