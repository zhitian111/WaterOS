#![no_std]
#![cfg_attr(not(feature = "enabled"), allow(dead_code, unused_imports))]
//! WaterOS 的版本化 GDB 诊断 ABI。
//!
//! 本 crate 只能依赖最底层配置。记录路径不得分配、打印或获取内核锁，否则锁死
//! 现场可能被调试器本身覆盖。主机端以 [`DEBUG_ABI_VERSION`] 判断布局兼容性。

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

pub const DEBUG_MAGIC : u64 = 0x5741_5445_5244_4247; // "WATERDBG"
pub const DEBUG_ABI_VERSION : u32 = 1;
pub const EVENT_CAPACITY : usize = 256;
pub const HELD_LOCK_CAPACITY : usize = 8;
pub const NO_TASK : u64 = u64::MAX;
pub const MAX_CPUS : usize = config::task::MAX_CPUS;
pub const ENABLED : bool = cfg!(feature = "enabled");
pub const DEBUG_FAULT_REASON_BASE : u32 = 0xF017_0000;

#[cfg(feature = "self_test")]
pub fn self_test() {
    assert_eq!(DEBUG_ABI_VERSION, 1);
    assert!(EVENT_CAPACITY > 0);
    assert!(HELD_LOCK_CAPACITY > 0);
}
#[cfg(target_arch = "riscv64")]
pub const DEBUG_ARCH : u16 = 1;
#[cfg(target_arch = "loongarch64")]
pub const DEBUG_ARCH : u16 = 2;
#[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
pub const DEBUG_ARCH : u16 = 0;

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

/// 为关键的 `spin::Mutex` 添加诊断而不改变调用处的 `lock()` 形状。
///
/// `current_cpu` 只在 `enabled` 构建调用；普通 release 会被常量折叠为一次原始
/// `Mutex::lock()`。字段析构顺序保证先真正解锁，再从诊断区删除 owner。
pub struct TrackedMutex<T> {
    inner : spin::Mutex<T>,
    kind : DebugLockKind,
    current_cpu : fn() -> usize,
}

impl<T> TrackedMutex<T> {
    pub const fn new(value : T, kind : DebugLockKind, current_cpu : fn() -> usize) -> Self {
        Self { inner : spin::Mutex::new(value), kind, current_cpu }
    }
}

impl<T> TrackedMutex<T> {
    #[inline]
    pub fn lock(&self) -> TrackedMutexGuard<'_, T> {
        if !ENABLED {
            return TrackedMutexGuard { guard : self.inner.lock(), _scope : None };
        }
        let cpu = (self.current_cpu)();
        let object = self as *const _ as *const () as usize;
        let guard = if let Some(guard) = self.inner.try_lock() {
            guard
        } else {
            lock_wait(cpu, 0, NO_TASK, self.kind, object);
            self.inner.lock()
        };
        lock_acquired(cpu, self.kind, object);
        TrackedMutexGuard { guard,
                            _scope : Some(DebugLockScope { cpu,
                                                           kind : self.kind,
                                                           object }) }
    }

    /// 不阻塞地尝试获取锁。失败不会重复记录 contention；需要轮询的调用方可
    /// 在第一次失败时显式调用 [`lock_wait`]。
    #[inline]
    pub fn try_lock(&self) -> Option<TrackedMutexGuard<'_, T>> {
        let guard = self.inner.try_lock()?;
        if !ENABLED {
            return Some(TrackedMutexGuard { guard, _scope : None });
        }
        let cpu = (self.current_cpu)();
        let object = self as *const _ as *const () as usize;
        lock_acquired(cpu, self.kind, object);
        Some(TrackedMutexGuard { guard,
                                 _scope : Some(DebugLockScope { cpu,
                                                                kind : self.kind,
                                                                object }) })
    }

    pub fn debug_identity(&self) -> (DebugLockKind, usize) {
        (self.kind, self as *const _ as *const () as usize)
    }
}

struct DebugLockScope {
    cpu : usize,
    kind : DebugLockKind,
    object : usize,
}

impl Drop for DebugLockScope {
    fn drop(&mut self) { lock_released(self.cpu, self.kind, self.object); }
}

pub struct TrackedMutexGuard<'a, T : ?Sized> {
    // Rust 按声明顺序析构字段：先释放真实 mutex，再发布诊断 release。
    guard : spin::MutexGuard<'a, T>,
    _scope : Option<DebugLockScope>,
}

impl<T : ?Sized> Deref for TrackedMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target { &self.guard }
}

impl<T : ?Sized> DerefMut for TrackedMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut self.guard }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct DebugLockRef {
    pub kind : u16,
    pub _reserved : [u16; 3],
    pub object : u64,
}

/// 一份已经完成组装、可原子发布的 CPU 语义状态。
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DebugCpuState {
    pub generation : u64,
    pub flags : u64,
    pub current_task : u64,
    pub current_address_space : u64,
    pub timer_ticks : u64,
    pub context_switches : u64,
    pub syscalls : u64,
    pub traps : u64,
    pub ipi_sent : u64,
    pub ipi_received : u64,
    pub last_trap_cause : u64,
    pub last_trap_pc : u64,
    pub last_trap_sp : u64,
    pub last_fault_addr : u64,
    pub last_syscall_nr : u64,
    pub last_syscall_pc : u64,
    pub runnable : [u32; 5],
    pub last_schedule_reason : u32,
    /// 0=unknown, 1=kernel, 2=user。
    pub task_kind : u16,
    /// 0=none, 1=ready, 2=running, 3=blocking, 4=sleeping, 5=exited。
    pub task_state : u16,
    /// Linux 调度策略原始值（OTHER=0/FIFO=1/RR=2/BATCH=3/IDLE=5）。
    pub sched_policy : u16,
    /// 0=none, 1=waitqueue, 2=task-exit, 3=child-exit, 4=manual,
    /// 5=sleep-until, 6=exit-code。
    pub wait_kind : u16,
    /// 等待对象 ID、唤醒 tick 或退出码，含义由 `wait_kind` 决定。
    pub wait_value : u64,
    pub nice : i8,
    pub _task_reserved : [u8; 7],
    pub waiting_lock : DebugLockRef,
    pub held_lock_count : u32,
    pub _reserved : u32,
    pub held_locks : [DebugLockRef; HELD_LOCK_CAPACITY],
}

impl DebugCpuState {
    pub const FLAG_ONLINE : u64 = 1 << 0;
    pub const FLAG_IDLE : u64 = 1 << 1;
    pub const FLAG_USER : u64 = 1 << 2;
    pub const FLAG_NEED_RESCHED : u64 = 1 << 3;

    pub const EMPTY : Self = Self { generation : 0,
                                    flags : 0,
                                    current_task : NO_TASK,
                                    current_address_space : 0,
                                    timer_ticks : 0,
                                    context_switches : 0,
                                    syscalls : 0,
                                    traps : 0,
                                    ipi_sent : 0,
                                    ipi_received : 0,
                                    last_trap_cause : 0,
                                    last_trap_pc : 0,
                                    last_trap_sp : 0,
                                    last_fault_addr : 0,
                                    last_syscall_nr : 0,
                                    last_syscall_pc : 0,
        runnable : [0; 5],
        last_schedule_reason : 0,
        task_kind : 0,
        task_state : 0,
        sched_policy : 0,
        wait_kind : 0,
        wait_value : 0,
        nice : 0,
        _task_reserved : [0; 7],
        waiting_lock : DebugLockRef { kind : 0,
                                                                  _reserved : [0; 3],
                                                                  object : 0 },
                                    held_lock_count : 0,
                                    _reserved : 0,
                                    held_locks : [DebugLockRef { kind : 0,
                                                                 _reserved : [0; 3],
                                                                 object : 0 };
                                                  HELD_LOCK_CAPACITY] };
}

#[repr(C)]
pub struct DebugCpuSlots {
    /// 0/1 表示最后完整发布的槽。
    pub published : AtomicUsize,
    /// 防止同一 CPU 的嵌套 trap 同时覆写非活动槽。
    pub writing : AtomicBool,
    pub dropped_updates : AtomicU64,
    slots : UnsafeCell<[DebugCpuState; 2]>,
}

unsafe impl Sync for DebugCpuSlots {}

impl DebugCpuSlots {
    const fn new() -> Self {
        Self { published : AtomicUsize::new(0),
               writing : AtomicBool::new(false),
               dropped_updates : AtomicU64::new(0),
               slots : UnsafeCell::new([DebugCpuState::EMPTY; 2]) }
    }

    /// 返回槽数组地址，仅供 debugger 按 ABI 读取。
    pub const fn slots_ptr(&self) -> *const DebugCpuState { self.slots.get().cast_const().cast() }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DebugEvent {
    pub tick : u64,
    pub task : u64,
    pub kind : u16,
    pub cpu : u16,
    pub flags : u32,
    pub caller_pc : u64,
    pub arg0 : u64,
    pub arg1 : u64,
    pub arg2 : u64,
}

impl DebugEvent {
    const EMPTY : Self = Self { tick : 0,
                                task : NO_TASK,
                                kind : 0,
                                cpu : 0,
                                flags : 0,
                                caller_pc : 0,
                                arg0 : 0,
                                arg1 : 0,
                                arg2 : 0 };
}

#[repr(C)]
pub struct DebugEventSlot {
    /// 最后以 Release 发布；0 表示该槽从未写入完成。
    pub sequence : AtomicU64,
    event : UnsafeCell<DebugEvent>,
}

unsafe impl Sync for DebugEventSlot {}

impl DebugEventSlot {
    const fn new() -> Self {
        Self { sequence : AtomicU64::new(0),
               event : UnsafeCell::new(DebugEvent::EMPTY) }
    }

    pub const fn event_ptr(&self) -> *const DebugEvent { self.event.get().cast_const() }
}

#[repr(C)]
pub struct DebugCpuEvents {
    pub next_sequence : AtomicU64,
    pub dropped_events : AtomicU64,
    pub slots : [DebugEventSlot; EVENT_CAPACITY],
}

impl DebugCpuEvents {
    const fn new() -> Self {
        Self { next_sequence : AtomicU64::new(1),
               dropped_events : AtomicU64::new(0),
               slots : [const { DebugEventSlot::new() }; EVENT_CAPACITY] }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DebugHeader {
    pub magic : u64,
    pub abi_version : u32,
    pub max_cpus : u16,
    pub event_capacity : u16,
    pub cpu_state_size : u32,
    pub event_size : u32,
    pub build_id_size : u32,
    /// 1=RISC-V64，2=LoongArch64。
    pub arch : u16,
    pub _reserved : u16,
    pub build_id : [u8; 64],
}

#[repr(C)]
pub struct DebugState {
    pub header : DebugHeader,
    pub cpus : [DebugCpuSlots; MAX_CPUS],
    pub events : [DebugCpuEvents; MAX_CPUS],
}

impl DebugState {
    const fn new() -> Self {
        Self { header : DebugHeader { magic : DEBUG_MAGIC,
                                      abi_version : DEBUG_ABI_VERSION,
                                      max_cpus : MAX_CPUS as u16,
                                      event_capacity : EVENT_CAPACITY as u16,
                                      cpu_state_size : size_of::<DebugCpuState>() as u32,
                                      event_size : size_of::<DebugEvent>() as u32,
                                      build_id_size : 64,
                                      arch : DEBUG_ARCH,
                                      _reserved : 0,
                                      build_id : build_id() },
               cpus : [const { DebugCpuSlots::new() }; MAX_CPUS],
               events : [const { DebugCpuEvents::new() }; MAX_CPUS] }
    }
}

const fn build_id() -> [u8; 64] {
    let source = match option_env!("WATEROS_DEBUG_BUILD_ID") {
        Some(value) => value.as_bytes(),
        None => b"development-build",
    };
    let mut out = [0u8; 64];
    let mut i = 0;
    while i < source.len() && i < out.len() - 1 {
        out[i] = source[i];
        i += 1;
    }
    out
}

#[cfg(feature = "enabled")]
#[unsafe(no_mangle)]
pub static WATEROS_DEBUG_BUILD_ID : [u8; 64] = build_id();

/// 由构建脚本写入的 frame-pointer 契约标记；doctor 会拒绝值为 0 的 ELF。
#[cfg(feature = "enabled")]
#[unsafe(no_mangle)]
pub static WATEROS_DEBUG_FRAME_POINTERS : u8 = cfg!(wateros_frame_pointers) as u8;

#[cfg(feature = "enabled")]
#[unsafe(no_mangle)]
pub static WATEROS_DEBUG_STATE : DebugState = DebugState::new();

/// 原子发布一份完整 CPU 状态。嵌套写入会被丢弃并计数，而不是自旋等待。
#[inline]
pub fn publish_cpu_state(cpu : usize, state : DebugCpuState) {
    #[cfg(feature = "enabled")]
    {
        let Some(cpu_slots) = WATEROS_DEBUG_STATE.cpus.get(cpu) else {
            return;
        };
        if cpu_slots.writing.compare_exchange(false,
                                               true,
                                               Ordering::Acquire,
                                               Ordering::Relaxed)
                            .is_err()
        {
            cpu_slots.dropped_updates.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let current = cpu_slots.published.load(Ordering::Relaxed) & 1;
        let next = current ^ 1;
        // SAFETY: writing CAS 保证同一时刻只有一个写入者；next 在发布前不可见。
        unsafe { (*cpu_slots.slots.get())[next] = state; }
        cpu_slots.published.store(next, Ordering::Release);
        cpu_slots.writing.store(false, Ordering::Release);
    }
    #[cfg(not(feature = "enabled"))]
    let _ = (cpu, state);
}

/// 在上一份完整状态的副本上执行原子字段更新，再发布新槽。
///
/// 闭包只在 `enabled` 构建执行，且不得调用任何可能阻塞、分配或打印的代码。
#[inline]
pub fn update_cpu_state(cpu : usize, update : impl FnOnce(&mut DebugCpuState)) {
    #[cfg(feature = "enabled")]
    {
        let Some(cpu_slots) = WATEROS_DEBUG_STATE.cpus.get(cpu) else {
            return;
        };
        if cpu_slots.writing.compare_exchange(false,
                                               true,
                                               Ordering::Acquire,
                                               Ordering::Relaxed)
                            .is_err()
        {
            cpu_slots.dropped_updates.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let current = cpu_slots.published.load(Ordering::Acquire) & 1;
        let next = current ^ 1;
        // SAFETY: writing CAS 排除其他写入者，current 是已经发布的只读槽。
        let mut state = unsafe { (*cpu_slots.slots.get())[current] };
        update(&mut state);
        state.generation = state.generation.wrapping_add(1);
        // SAFETY: next 尚未发布，只有当前写入者可访问。
        unsafe { (*cpu_slots.slots.get())[next] = state; }
        cpu_slots.published.store(next, Ordering::Release);
        cpu_slots.writing.store(false, Ordering::Release);
    }
    #[cfg(not(feature = "enabled"))]
    let _ = (cpu, update);
}

/// 向目标 CPU 的固定大小事件环追加记录。
#[inline]
pub fn record_event(cpu : usize,
                    tick : u64,
                    task : u64,
                    kind : DebugEventKind,
                    caller_pc : usize,
                    args : [u64; 3]) {
    #[cfg(feature = "enabled")]
    {
        // 保留独立 build-id 符号，供主机在读取/解释任何地址前校验 ELF。
        let _ = core::hint::black_box(&WATEROS_DEBUG_BUILD_ID);
        let _ = core::hint::black_box(&WATEROS_DEBUG_FRAME_POINTERS);
        let Some(events) = WATEROS_DEBUG_STATE.events.get(cpu) else {
            return;
        };
        let sequence = events.next_sequence.fetch_add(1, Ordering::Relaxed);
        if sequence > EVENT_CAPACITY as u64 {
            // 诊断环没有消费指针；覆盖一条旧记录时累计 overflow，报告可据此
            // 明确提示时间线只保留了最近窗口。
            events.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
        let index = sequence as usize % EVENT_CAPACITY;
        let slot = &events.slots[index];
        // SAFETY: sequence 为每个写入者分配唯一槽次；旧记录只有在 sequence 最后
        // 发布后才有效，主机发现 sequence 不匹配时会忽略正在覆写的槽。
        unsafe {
            *slot.event.get() = DebugEvent { tick,
                                             task,
                                             kind : kind as u16,
                                             cpu : cpu as u16,
                                             flags : 0,
                                             caller_pc : caller_pc as u64,
                                             arg0 : args[0],
                                             arg1 : args[1],
                                             arg2 : args[2] };
        }
        slot.sequence.store(sequence, Ordering::Release);
    }
    #[cfg(not(feature = "enabled"))]
    let _ = (cpu, tick, task, kind, caller_pc, args);
}

/// 标记当前 CPU 正在等待一个关键锁，并记录一次 contention。
#[inline]
pub fn lock_wait(cpu : usize,
                 tick : u64,
                 task : u64,
                 kind : DebugLockKind,
                 object : usize) {
    update_cpu_state(cpu, |state| {
        state.waiting_lock.kind = kind as u16;
        state.waiting_lock.object = object as u64;
    });
    record_event(cpu,
                 tick,
                 task,
                 DebugEventKind::LockContended,
                 0,
                 [kind as u64, object as u64, 0]);
}

/// 将锁从 waiting 移入当前 CPU 的固定容量 held-lock 集合。
#[inline]
pub fn lock_acquired(cpu : usize, kind : DebugLockKind, object : usize) {
    update_cpu_state(cpu, |state| {
        state.waiting_lock = DebugLockRef::default();
        let index = state.held_lock_count as usize;
        if index < HELD_LOCK_CAPACITY {
            state.held_locks[index].kind = kind as u16;
            state.held_locks[index].object = object as u64;
            state.held_lock_count += 1;
        }
    });
    record_event(cpu,
                 0,
                 NO_TASK,
                 DebugEventKind::LockAcquire,
                 0,
                 [kind as u64, object as u64, 0]);
}

/// 删除一项已持有锁；允许非 LIFO guard drop，避免诊断状态长期残留。
#[inline]
pub fn lock_released(cpu : usize, kind : DebugLockKind, object : usize) {
    update_cpu_state(cpu, |state| {
        let count = (state.held_lock_count as usize).min(HELD_LOCK_CAPACITY);
        let mut found = None;
        let mut index = 0;
        while index < count {
            let lock = state.held_locks[index];
            if lock.kind == kind as u16 && lock.object == object as u64 {
                found = Some(index);
                break;
            }
            index += 1;
        }
        if let Some(found) = found {
            let mut cursor = found;
            while cursor + 1 < count {
                state.held_locks[cursor] = state.held_locks[cursor + 1];
                cursor += 1;
            }
            state.held_locks[count - 1] = DebugLockRef::default();
            state.held_lock_count = (count - 1) as u32;
        }
    });
    record_event(cpu,
                 0,
                 NO_TASK,
                 DebugEventKind::LockRelease,
                 0,
                 [kind as u64, object as u64, 0]);
}
