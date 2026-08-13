//! GDB 诊断事件与锁类别的稳定编号。

/// 稳定的事件编号；只能追加，不能复用旧编号。
#[repr(u16)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugEventKind {
    TaskEnqueue = 1,
    TaskSwitch = 2,
    TaskBlock = 3,
    TaskWake = 4,
    TaskExit = 5,
    SyscallEnter = 6,
    SyscallExit = 7,
    TrapEnter = 8,
    TrapExit = 9,
    Timer = 10,
    IpiSend = 11,
    IpiReceive = 12,
    FutexWait = 13,
    FutexWake = 14,
    TlbShootdown = 15,
    LockContended = 16,
    LockAcquire = 17,
    LockRelease = 18,
}

/// 主机报告中显示的关键锁类别。
#[repr(u16)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DebugLockKind {
    #[default]
    None = 0,
    Scheduler = 1,
    ProcessRegistry = 2,
    FutexRegistry = 3,
    FrameAllocator = 4,
    AddressSpace = 5,
    Vfs = 6,
    Network = 7,
    Klog = 8,
}
