#![no_std]

/// 任务在系统内的唯一标识。
pub type TaskId = usize;
/// 调度器使用的逻辑时钟单位。
pub type TaskTick = u64;
/// 任务退出时返回给上层的状态码。
pub type TaskExitCode = isize;
/// 等待队列在调度器中的唯一标识。
pub type WaitQueueId = usize;
/// 内核任务入口函数签名。
pub type KernelTaskEntry = extern "C" fn(usize) -> !;

/// 预留给 idle 任务的固定任务号。
pub const IDLE_TASK_ID: TaskId = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    /// 只在内核态运行的任务。
    Kernel,
    /// 后续用于承载用户态任务。
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskBlockReason {
    /// 主动让出 CPU。
    Yield,
    /// 因定时睡眠而阻塞。
    Sleep,
    /// 因等待某个可阻塞对象而休眠。
    Wait(TaskWaitHandle),
    /// 因系统调用路径暂时挂起。
    UserSyscall,
    /// 由内核显式置为阻塞。
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    /// 已就绪，等待被调度运行。
    Ready,
    /// 当前正在 CPU 上运行。
    Running,
    /// 由于某种阻塞原因暂时不可运行。
    Blocking(TaskBlockReason),
    /// 睡眠到指定 tick 后再尝试唤醒。
    Sleeping { wake_tick: TaskTick },
    /// 已退出，不会再被调度。
    Exited(TaskExitCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScheduleReason {
    /// 第一次切入任务系统。
    StartFirst,
    /// 当前任务主动让出 CPU。
    Yield,
    /// 由时钟 tick 触发一次调度检查。
    Tick,
    /// 由于阻塞而切换出去。
    Block(TaskBlockReason),
    /// 由于定时睡眠而切换出去。
    Sleep(TaskTick),
    /// 当前任务退出。
    Exit(TaskExitCode),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskWaitResult {
    /// 等待对象正常唤醒了任务。
    Woken,
    /// 超时时间先到，任务因超时返回。
    TimedOut,
}

/// 可被任务等待的目标对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskWaitTarget {
    /// 等待某个显式 wait queue。
    WaitQueue(WaitQueueId),
    /// 等待某个任务进入退出状态。
    TaskExit(TaskId),
}

/// 对一个可等待对象的稳定引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskWaitHandle {
    target: TaskWaitTarget,
}

impl TaskWaitHandle {
    /// 为指定 wait queue 构造等待句柄。
    #[inline]
    pub const fn for_wait_queue(wait_queue_id: WaitQueueId) -> Self {
        Self {
            target: TaskWaitTarget::WaitQueue(wait_queue_id),
        }
    }

    /// 为指定任务退出事件构造等待句柄。
    #[inline]
    pub const fn for_task_exit(task_id: TaskId) -> Self {
        Self {
            target: TaskWaitTarget::TaskExit(task_id),
        }
    }

    /// 返回该等待句柄指向的目标对象。
    #[inline]
    pub const fn target(&self) -> TaskWaitTarget { self.target }
}

/// 调度器为任务维护的基础运行统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskRuntimeStats {
    /// 该任务累计被切入运行的次数。
    pub schedule_count: usize,
    /// 该任务累计消耗的 tick 数。
    pub tick_count: usize,
}

/// 任务自己持有的最近一次 trap 上下文快照。
///
/// 布局刻意与当前 RISC-V `TrapContext` 保持一致，方便 trap 路径直接
/// 整体复制，而不需要逐字段转换。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TaskTrapFrame {
    pub x: [usize; 32],
    pub sstatus: usize,
    pub sepc: usize,
    pub scause: usize,
    pub stval: usize,
}

const RISCV_SSTATUS_SIE: usize = 1 << 1;
const RISCV_SSTATUS_SPIE: usize = 1 << 5;
const RISCV_SSTATUS_SPP: usize = 1 << 8;

impl TaskTrapFrame {
    /// 返回原始 trap 原因编码。
    #[inline]
    pub const fn raw_cause(&self) -> usize { self.scause }

    /// 返回发生 trap 时保存的程序计数器。
    #[inline]
    pub const fn user_pc(&self) -> usize { self.sepc }

    /// 返回发生 trap 时保存的用户栈指针。
    #[inline]
    pub const fn user_sp(&self) -> usize { self.x[2] }

    /// 返回与 trap 关联的故障地址或附加值。
    #[inline]
    pub const fn fault_addr(&self) -> usize { self.stval }

    /// 判断当前 trap frame 在恢复时是否会返回到用户态。
    #[inline]
    pub const fn returns_to_user(&self) -> bool { (self.sstatus & RISCV_SSTATUS_SPP) == 0 }

    /// 判断当前 trap frame 在恢复时是否会返回到内核态。
    #[inline]
    pub const fn returns_to_kernel(&self) -> bool { !self.returns_to_user() }

    /// 设置恢复后的用户 PC。
    #[inline]
    pub fn set_user_pc(&mut self, pc: usize) { self.sepc = pc; }

    /// 在当前用户 PC 基础上前进指定字节数。
    #[inline]
    pub fn add_user_pc(&mut self, bytes: usize) {
        self.sepc = self.sepc.wrapping_add(bytes);
    }

    /// 设置恢复后的用户栈指针。
    #[inline]
    pub fn set_user_sp(&mut self, sp: usize) { self.x[2] = sp; }

    /// 设置 syscall 返回值寄存器。
    #[inline]
    pub fn set_syscall_ret(&mut self, ret: isize) { self.x[10] = ret as usize; }

    /// 将该 trap frame 标记为恢复到用户态。
    #[inline]
    pub fn set_return_to_user(&mut self) {
        self.sstatus &= !RISCV_SSTATUS_SPP;
        self.sstatus &= !RISCV_SSTATUS_SIE;
        self.sstatus |= RISCV_SSTATUS_SPIE;
    }

    /// 将该 trap frame 标记为恢复到内核态。
    #[inline]
    pub fn set_return_to_kernel(&mut self) { self.sstatus |= RISCV_SSTATUS_SPP; }

    /// 准备一次最小的用户态返回现场。
    #[inline]
    pub fn prepare_user_return(&mut self, entry_pc: usize, user_sp: usize) {
        self.set_user_pc(entry_pc);
        self.set_user_sp(user_sp);
        self.set_return_to_user();
    }
}

/// 对外暴露的稳定任务快照。
///
/// 这里故意不暴露内核栈地址、bootstrap 协议细节和保存上下文布局，
/// 让公共 API 更偏语义，而不是直接泄漏实现形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSnapshot {
    /// 任务号。
    pub id: TaskId,
    /// 任务类别。
    pub kind: TaskKind,
    /// 当前任务状态。
    pub state: TaskState,
    /// 最近一次 trap 的保存现场。
    pub trap_frame: Option<TaskTrapFrame>,
    /// 调度器维护的运行统计。
    pub stats: TaskRuntimeStats,
}

/// 已退出任务的可回收信息。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExitedTask {
    /// 任务号。
    pub id: TaskId,
    /// 任务类别。
    pub kind: TaskKind,
    /// 退出状态码。
    pub exit_code: TaskExitCode,
    /// 退出前最后一次 trap 现场。
    pub trap_frame: Option<TaskTrapFrame>,
    /// 退出时刻的运行统计。
    pub stats: TaskRuntimeStats,
}
