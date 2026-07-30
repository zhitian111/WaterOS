//! 任务控制块（**`TaskControlBlock`**）与任务类型专属资源：把 `task_api`
//! 中的规格落到具体栈、trap 帧与地址空间句柄上。
//!
//! 调度器只通过 `task_api` 抽象操作本模块类型。

use abi::user_ret::UserRet;
use alloc::boxed::Box;
use api_v0::{
    AddressSpaceHandle, CpuId, CpuMask, ExitedTask, KernelStack, KernelTaskEntry, SchedPolicy,
    TaskBootstrap, TaskExitCode, TaskId, TaskKind, TaskRuntimeStats, TaskSnapshot, TaskState,
    TaskTick, TaskTrapSnapshot, TaskWaitResult, TaskWaitTarget, UserImageInfo, UserStack, UserTask,
    VRunTime,
};
use arch::task::{ActiveArchTaskContext as TaskContext, ArchTaskContext};
use arch::trap::{ActiveTrapFrame as TaskTrapFrame, TrapFrameRead, TrapFrameWrite};

unsafe extern "C" {
    fn __arch_task_entry();
    fn __arch_user_task_entry();
}

// ── 任务类型专属资源 ──────────────────────────────────────────────

enum TaskInner {
    /// Idle 须与 [`KernelResources`] 一样持有内核栈与 bootstrap，否则
    /// `task_cx.sp` / `s[0]` 会指向创建后立即 drop
    /// 的堆内存，堆复用后上下文损坏。
    Idle(KernelResources),
    Kernel(KernelResources),
    User(UserResources),
}

struct KernelResources {
    kernel_stack : KernelStack,
    bootstrap : Box<TaskBootstrap>,
}

struct UserResources {
    kernel_stack : KernelStack,
    trap_frame : TaskTrapFrame,
    user : UserTask,
}

impl UserResources {
    fn new(kernel_stack : KernelStack, user : UserTask) -> Self {
        let token = user.address_space()
                        .expect("[wateros-task]user task requires an address space ")
                        .raw();
        let entry_pc = user.entry_pc();
        let stack = user.stack()
                        .expect("[wateros-task]UserTask must have a user stack");
        let user_sp = user.initial_user_sp()
                          .unwrap_or_else(|| initial_user_sp(stack.top(), stack.bottom()));
        let mut trap_frame = TaskTrapFrame::default();
        //构造返回用户态时的trap
        trap_frame.prepare_user_return(entry_pc, user_sp);
        // 设置首次进入用户态时的 argc/argv/envp
        if let Some((argc, argv, envp)) = user.initial_user_args() {
            trap_frame.set_user_entry_args(argc, argv, envp);
        }
        // 设置 trap frame 的返回地址空间 token
        trap_frame.set_return_address_space_token(token);
        Self { kernel_stack,
               trap_frame,
               user }
    }
}

// ── 辅助函数 ─────────────────────────────────────────────────────

fn initial_user_sp(top_exclusive : usize, bottom : usize) -> usize {
    let sp = top_exclusive.saturating_sub(16);
    if sp < bottom {
        bottom
    } else {
        sp
    }
}

fn trap_snapshot(trap_frame : TaskTrapFrame, user_aspace_ptr : usize) -> TaskTrapSnapshot {
    TaskTrapSnapshot::new(trap_frame.raw_cause(),
                          trap_frame.user_pc(),
                          trap_frame.user_sp(),
                          user_aspace_ptr,
                          trap_frame.fault_addr(),
                          trap_frame.returns_to_user())
}

// ── 任务控制块 ───────────────────────────────────────────────────

/// 调度器持有的任务控制块。
pub struct TaskControlBlock {
    id : TaskId,
    parent_id : Option<TaskId>,
    state : TaskState,
    policy : SchedPolicy,
    priority : i32,
    /// fair 调度类（OTHER/BATCH/IDLE）的线程级 nice 属性。
    ///
    /// 调度器在 tick 时据此计算 vruntime 增量；该字段是唯一的 task-level 真相，
    /// 避免把线程调度属性错误地存成 process-wide 状态。
    nice : i8,
    /// fair 调度类的累计虚拟运行时间。跨 CPU 迁移时仍由 TCB 保持。
    vruntime : VRunTime,
    stats : TaskRuntimeStats,
    wait_result : Option<TaskWaitResult>,
    task_cx : TaskContext,
    inner : TaskInner,
    /// The CPU runqueue that owns this task while it is published as Ready.
    /// `None` is also used for a newly created task that has not been queued.
    ready_cpu_id : Option<CpuId>,
    /// Most recent CPU that executed this task; retained across blocking.
    last_cpu_id : Option<CpuId>,
    /// Sole source of truth for the CPU while `state == Running`.
    running_cpu_id : Option<CpuId>,
    /// CPUs on which this task may run. The scheduler further intersects this
    /// with its configured and online CPU sets before selecting a runqueue.
    affinity : CpuMask,
}

impl TaskControlBlock {
    /// 创建一个普通内核任务，并初始化其启动上下文。
    pub fn new_kernel_task(id : TaskId,
                           parent_id : Option<TaskId>,
                           entry : KernelTaskEntry,
                           arg : usize)
                           -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, arg));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(__arch_task_entry as *const () as usize,
                                                   kernel_stack.top(),
                                                   bootstrap_ptr);
        Self { id,
               parent_id,
               state : TaskState::Ready,
               policy : SchedPolicy::Other,
               priority : 0,
               nice : 0,
               vruntime : 0,
               stats : TaskRuntimeStats::default(),
               wait_result : None,
               task_cx,
               inner : TaskInner::Kernel(KernelResources { kernel_stack,
                                                           bootstrap }),
               ready_cpu_id : None,
               last_cpu_id : None,
               running_cpu_id : None,
               affinity : CpuMask::ALL }
    }

    /// 创建 idle 任务。
    pub fn new_idle_task(id : TaskId, entry : KernelTaskEntry) -> Self {
        let kernel_stack = KernelStack::new();
        let bootstrap = Box::new(TaskBootstrap::new(entry, 0));
        let bootstrap_ptr = bootstrap.as_ref() as *const TaskBootstrap as usize;
        let task_cx = TaskContext::goto_task_entry(__arch_task_entry as *const () as usize,
                                                   kernel_stack.top(),
                                                   bootstrap_ptr);
        Self { id,
               parent_id : None,
               state : TaskState::Ready,
               policy : SchedPolicy::Other,
               priority : 0,
               nice : 0,
               vruntime : 0,
               stats : TaskRuntimeStats::default(),
               wait_result : None,
               task_cx,
               inner : TaskInner::Idle(KernelResources { kernel_stack,
                                                         bootstrap }),
               ready_cpu_id : None,
               last_cpu_id : None,
               running_cpu_id : None,
               affinity : CpuMask::ALL }
    }

    /// 创建一个用户任务。
    pub fn new_user_task(id : TaskId, parent_id : Option<TaskId>, user : UserTask) -> Self {
        let kernel_stack = KernelStack::new();
        let task_cx = TaskContext::goto_entry(__arch_user_task_entry as *const () as usize,
                                              kernel_stack.top());
        let user = UserResources::new(kernel_stack, user);
        Self { id,
               parent_id,
               state : TaskState::Ready,
               policy : SchedPolicy::Other,
               priority : 0,
               nice : 0,
               vruntime : 0,
               stats : TaskRuntimeStats::default(),
               wait_result : None,
               task_cx,
               inner : TaskInner::User(user),
               ready_cpu_id : None,
               last_cpu_id : None,
               running_cpu_id : None,
               affinity : CpuMask::ALL }
    }

    /// 从父任务 fork 一个子用户任务。独立地址空间、独立 trap frame、独立内核栈，但共享父任务的用户映像。
    pub fn fork_from(&self,
                     child_id : TaskId,
                     child_parent_id : TaskId,
                     child_stack : usize,
                     new_aspace_ptr : usize,
                     new_satp : usize)
                     -> Option<Self> {
        // 只复制用户任务
        let parent_user = match &self.inner {
            TaskInner::User(u) => u,
            _ => return None,
        };

        // 复制父 trap 帧
        let mut child_trap = parent_user.trap_frame;
        // fork的子进程返回0
        child_trap.set_syscall_ret(UserRet::from_success(0));
        // 执行下一条指令
        child_trap.add_user_pc(4);
        // 指定子任务的用户栈指针
        //可选是否使用新栈
        child_trap.set_return_address_space_token(new_satp);
        if child_stack != 0 {
            child_trap.set_user_sp(child_stack);
        }
        let parent_spec = &parent_user.user;

        // 构造 UserTask
        let child_spec = UserTask::new(parent_spec.entry_pc(),
                                       AddressSpaceHandle::from_raw(new_satp),
                                       parent_spec.image()
                                                  .expect("parent user task must have image"),
                                       parent_spec.stack()
                                                  .expect("parent user task must have stack"),
                                       new_aspace_ptr);

        let kernel_stack = KernelStack::try_new()?;
        let task_cx = TaskContext::goto_entry(__arch_user_task_entry as *const () as usize,
                                              kernel_stack.top());
        Some(Self { id : child_id,
                    parent_id : Some(child_parent_id),
                    state : TaskState::Ready,
                    policy : self.policy,
                    priority : self.priority,
                    nice : self.nice,
                    vruntime : self.vruntime,
                    stats : TaskRuntimeStats::default(),
                    wait_result : None,
                    task_cx,
                    inner : TaskInner::User(UserResources { kernel_stack,
                                                            trap_frame : child_trap,
                                                            user : child_spec }),
                    ready_cpu_id : None,
                    last_cpu_id : None,
                    running_cpu_id : None,
                    affinity : self.affinity })
    }

    /// 从当前用户任务 clone 一个同进程线程。共享地址空间
    pub fn clone_thread_from(&self,
                             child_id : TaskId,
                             child_stack : usize,
                             tls : usize,
                             set_tls : bool)
                             -> Option<Self> {
        let parent_user = match &self.inner {
            TaskInner::User(u) => u,
            _ => return None,
        };

        let mut child_trap = parent_user.trap_frame;
        child_trap.set_syscall_ret(UserRet::from_success(0));
        child_trap.add_user_pc(4);
        if child_stack != 0 {
            child_trap.set_user_sp(child_stack);
        }
        if set_tls {
            child_trap.set_user_tls(tls);
        }
        let kernel_stack = KernelStack::try_new()?;
        let task_cx = TaskContext::goto_entry(__arch_user_task_entry as *const () as usize,
                                              kernel_stack.top());
        Some(Self { id : child_id,
                    parent_id : Some(self.id),
                    state : TaskState::Ready,
                    policy : self.policy,
                    priority : self.priority,
                    nice : self.nice,
                    vruntime : self.vruntime,
                    stats : TaskRuntimeStats::default(),
                    wait_result : None,
                    task_cx,
                    inner : TaskInner::User(UserResources { kernel_stack,
                                                            trap_frame : child_trap,
                                                            user : parent_user.user }),
                    ready_cpu_id : None,
                    last_cpu_id : None,
                    running_cpu_id : None,
                    affinity : self.affinity })
    }

    /// execve：替换当前任务的地址空间、栈和入口。
    ///
    /// - 销毁旧 `UserResources`（内核栈、旧地址空间）
    /// - 安装新的 `entry_pc`、`sp`、`satp`、`user_aspace_ptr`
    pub fn execve_from(&mut self,
                       entry_pc : usize,
                       sp : usize,
                       argc : usize,
                       argv : usize,
                       envp : usize,
                       satp : usize,
                       user_aspace_ptr : usize,
                       image_info : UserImageInfo,
                       stack_info : UserStack) {
        let user_inner = match &mut self.inner {
            TaskInner::User(u) => u,
            _ => return,
        };

        // 构造新 UserTask 规格
        let new_spec = UserTask::new(entry_pc,
                                     AddressSpaceHandle::from_raw(satp),
                                     image_info,
                                     stack_info,
                                     user_aspace_ptr).with_initial_user_sp(sp)
                                                     .with_initial_user_args(argc, argv, envp);

        // 新 trap 帧：入口 + 用户栈 + satp（trap_handler 对 execve 跳过 add_user_pc）
        let mut new_trap = TaskTrapFrame::default();
        new_trap.prepare_user_return(entry_pc, sp);
        new_trap.set_user_entry_args(argc, argv, envp);
        new_trap.set_return_address_space_token(satp);
        user_inner.trap_frame = new_trap;
        user_inner.user = new_spec;
    }

    // ── 通用访问器 ──────────────────────────────────────────────

    #[inline]
    pub fn id(&self) -> TaskId { self.id }

    #[inline]
    pub fn parent_id(&self) -> Option<TaskId> { self.parent_id }

    #[inline]
    pub fn state(&self) -> TaskState { self.state }

    #[inline]
    pub fn ready_cpu_id(&self) -> Option<CpuId> { self.ready_cpu_id }

    #[inline]
    pub fn last_cpu_id(&self) -> Option<CpuId> { self.last_cpu_id }

    #[inline]
    pub fn running_cpu_id(&self) -> Option<CpuId> { self.running_cpu_id }

    #[inline]
    pub fn policy(&self) -> SchedPolicy { self.policy }

    #[inline]
    pub fn priority(&self) -> i32 { self.priority }

    #[inline]
    pub fn nice(&self) -> i8 { self.nice }

    #[inline]
    pub fn set_sched(&mut self, policy : SchedPolicy, priority : i32) {
        self.policy = policy;
        self.priority = priority;
    }

    #[inline]
    pub fn set_nice(&mut self, nice : i8) { self.nice = nice; }
    pub fn set_affinity(&mut self, mask : CpuMask) { self.affinity = mask; }

    #[inline]
    pub fn affinity(&self) -> CpuMask { self.affinity }
    #[inline]
    pub fn is_idle(&self) -> bool { matches!(self.inner, TaskInner::Idle(_)) }

    #[inline]
    pub fn is_user(&self) -> bool { matches!(self.inner, TaskInner::User(_)) }

    #[inline]
    pub fn context_ptr(&self) -> *const TaskContext { &self.task_cx as *const TaskContext }

    #[inline]
    pub fn context_mut_ptr(&mut self) -> *mut TaskContext { &mut self.task_cx as *mut TaskContext }

    #[inline]
    pub fn snapshot(&self) -> TaskSnapshot {
        let kind = match &self.inner {
            TaskInner::Idle(_) | TaskInner::Kernel(_) => TaskKind::Kernel,
            TaskInner::User(_) => TaskKind::User,
        };
        let trap_frame = match &self.inner {
            TaskInner::User(u) => Some(trap_snapshot(u.trap_frame,
                                                     u.user
                                                      .user_aspace_ptr()
                                                      .unwrap_or(0))),
            _ => None,
        };
        TaskSnapshot { id : self.id,
                       parent_id : self.parent_id,
                       kind,
                       state : self.state,
                       policy : self.policy,
                       priority : self.priority,
                       nice : self.nice,
                       vruntime : self.vruntime,
                       trap_frame,
                       stats : self.stats,
                       ready_cpu_id : self.ready_cpu_id,
                       running_cpu_id : self.running_cpu_id,
                       last_cpu_id : self.last_cpu_id,
                       affinity : self.affinity,
                       user_aspace_ptr : self.user_aspace_ptr(),
                       task_cx : self.context_ptr() as *const () }
    }

    // ── 内核栈 ──────────────────────────────────────────────────

    #[inline]
    pub fn kernel_stack_top(&self) -> usize {
        match &self.inner {
            TaskInner::Idle(k) | TaskInner::Kernel(k) => k.kernel_stack.top(),
            TaskInner::User(u) => u.kernel_stack.top(),
        }
    }

    #[inline]
    pub fn set_vruntime(&mut self, vruntime : VRunTime) { self.vruntime = vruntime; }

    // ── 用户任务方法 ─────────────────────────────────────────────

    #[inline]
    pub fn user_stack_top(&self) -> Option<usize> {
        match &self.inner {
            TaskInner::User(u) => u.user
                                   .stack()
                                   .map(|s| s.top()),
            _ => None,
        }
    }

    #[inline]
    pub fn user_address_space_raw(&self) -> usize {
        match &self.inner {
            TaskInner::User(u) => u.user
                                   .address_space()
                                   .map(|h| h.raw())
                                   .unwrap_or(0),
            _ => 0,
        }
    }

    #[inline]
    pub fn user_aspace_ptr(&self) -> usize {
        match &self.inner {
            TaskInner::User(u) => u.user
                                   .user_aspace_ptr()
                                   .unwrap_or(0),
            _ => 0,
        }
    }

    #[inline]
    pub fn trap_return_address_space_token(&self) -> usize {
        match &self.inner {
            TaskInner::User(u) => TrapFrameRead::return_address_space_token(&u.trap_frame),
            _ => 0,
        }
    }

    // ── Bootstrap（内核任务首次启动用） ──────────────────────────

    #[inline]
    pub fn bootstrap_ptr(&self) -> Option<usize> {
        match &self.inner {
            TaskInner::Kernel(k) => Some(k.bootstrap.as_ref() as *const TaskBootstrap as usize),
            _ => None,
        }
    }

    // ── Trap frame 访问 ─────────────────────────────────────────

    #[inline]
    pub fn begin_trap_frame_access(&mut self, trap_frame : TaskTrapFrame) -> *mut TaskTrapFrame {
        match &mut self.inner {
            TaskInner::User(u) => {
                u.trap_frame = trap_frame;
                &mut u.trap_frame as *mut TaskTrapFrame
            }
            _ => panic!("begin_trap_frame_access called on non-user task"),
        }
    }

    #[inline]
    pub fn restore_trap_frame_into(&self, trap_frame : &mut TaskTrapFrame) -> bool {
        match &self.inner {
            TaskInner::User(u) => {
                *trap_frame = u.trap_frame;
                true
            }
            _ => false,
        }
    }

    // ── 退出 ────────────────────────────────────────────────────

    #[inline]
    pub fn exited_task(&self) -> Option<ExitedTask> {
        let TaskState::Exited(exit_code) = self.state else {
            return None;
        };
        let kind = match &self.inner {
            TaskInner::Idle(_) | TaskInner::Kernel(_) => TaskKind::Kernel,
            TaskInner::User(_) => TaskKind::User,
        };
        let trap_frame = match &self.inner {
            TaskInner::User(u) => Some(trap_snapshot(u.trap_frame,
                                                     u.user
                                                      .user_aspace_ptr()
                                                      .unwrap_or(0))),
            _ => None,
        };
        Some(ExitedTask { id : self.id,
                          parent_id : self.parent_id,
                          kind,
                          exit_code,
                          trap_frame,
                          stats : self.stats })
    }

    // ── 状态变迁 ────────────────────────────────────────────────

    #[inline]
    pub fn mark_ready(&mut self, cpu_id : CpuId) {
        self.state = TaskState::Ready;
        self.ready_cpu_id = Some(cpu_id);
        self.running_cpu_id = None;
    }

    #[inline]
    pub fn mark_running(&mut self, cpu_id : CpuId) {
        self.state = TaskState::Running;
        self.ready_cpu_id = None;
        // Idle task 不属于普通任务的运行归属模型：它不会进入 ready queue，
        // 也不应以 `running_cpu_id` 参与跨 CPU 任务唯一性判断。
        if self.is_idle() {
            self.running_cpu_id = None;
        } else {
            self.running_cpu_id = Some(cpu_id);
            self.last_cpu_id = Some(cpu_id);
        }
        self.stats
            .schedule_count = self.stats
                                  .schedule_count
                                  .saturating_add(1);
    }

    #[inline]
    pub fn mark_blocking(&mut self, reason : TaskWaitTarget) {
        self.state = TaskState::Blocking(reason);
        self.ready_cpu_id = None;
        self.running_cpu_id = None;
    }

    #[inline]
    pub fn mark_sleeping(&mut self, wake_tick : TaskTick) {
        self.state = TaskState::Sleeping { wake_tick };
        self.ready_cpu_id = None;
        self.running_cpu_id = None;
    }

    #[inline]
    pub fn mark_exited(&mut self, exit_code : TaskExitCode) {
        if let TaskInner::User(u) = &mut self.inner {
            // 用户页表由进程 registry 在 reap_process 时统一释放；线程 exit
            // 时仅丢弃 TCB 内句柄，避免 CLONE_VM 共享 aspace 被提前 destroy。
            u.user = u.user
                      .without_user_aspace();
        }
        self.state = TaskState::Exited(exit_code);
        self.ready_cpu_id = None;
        self.running_cpu_id = None;
    }

    #[inline]
    pub fn tick(&mut self) {
        self.add_ticks(1);
    }

    #[inline]
    pub fn add_ticks(&mut self, ticks : u64) {
        let ticks = ticks.min(usize::MAX as u64) as usize;
        self.stats
            .tick_count = self.stats
                              .tick_count
                              .saturating_add(ticks);
    }

    // ── 等待 ────────────────────────────────────────────────────

    #[inline]
    pub fn clear_wait_result(&mut self) { self.wait_result = None; }

    #[inline]
    pub fn finish_wait(&mut self, result : TaskWaitResult) { self.wait_result = Some(result); }

    #[inline]
    pub fn take_wait_result(&mut self) -> TaskWaitResult {
        self.wait_result
            .take()
            .unwrap_or(TaskWaitResult::Woken)
    }

    #[inline]
    pub fn ready_to_wake(&self, current_tick : TaskTick) -> bool {
        matches!(self.state, TaskState::Sleeping { wake_tick } if wake_tick <= current_tick)
    }
}

// ----- end of TaskControlBlock impl -----
