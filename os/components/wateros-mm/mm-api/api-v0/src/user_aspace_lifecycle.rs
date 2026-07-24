//! 用户地址空间在任务退出时的释放钩子（由 mm-impl 注册，task 在 `exit` 时调用）。

/// 释放 `user_aspace_ptr` 指向的用户页表与映射帧。
pub type DropUserAspaceFn = fn(usize);
pub type AspaceCpuHook = fn(usize, base::cpu::CpuId);

static DROP_USER_ASPACE: spin::Mutex<Option<DropUserAspaceFn>> = spin::Mutex::new(None);
static ASPACE_CPU_ENTER: spin::Mutex<Option<AspaceCpuHook>> = spin::Mutex::new(None);
static ASPACE_CPU_LEAVE: spin::Mutex<Option<AspaceCpuHook>> = spin::Mutex::new(None);

/// 由 `mm::kernel_mm::init` 注册；未注册时 [`drop_user_aspace_on_task_exit`] 为 no-op。
pub fn register_drop_user_aspace_hook(f: DropUserAspaceFn) {
    *DROP_USER_ASPACE.lock() = Some(f);
}

/// 任务 `exit` 路径调用：立即释放地址空间（子进程、busybox、被 kill 的任务均走此路径）。
pub fn drop_user_aspace_on_task_exit(aspace_ptr: usize) {
    if aspace_ptr == 0 {
        return;
    }
    let hook = *DROP_USER_ASPACE.lock();
    if let Some(f) = hook {
        f(aspace_ptr);
    }
}

/// Register callbacks used by the scheduler to track CPUs currently using an
/// address space.  The callbacks are optional for dummy and single-core MM.
pub fn register_aspace_cpu_hooks(enter: AspaceCpuHook, leave: AspaceCpuHook) {
    *ASPACE_CPU_ENTER.lock() = Some(enter);
    *ASPACE_CPU_LEAVE.lock() = Some(leave);
}

pub fn notify_aspace_cpu_enter(aspace_ptr: usize, cpu: base::cpu::CpuId) {
    if aspace_ptr == 0 { return; }
    if let Some(f) = *ASPACE_CPU_ENTER.lock() { f(aspace_ptr, cpu); }
}

pub fn notify_aspace_cpu_leave(aspace_ptr: usize, cpu: base::cpu::CpuId) {
    if aspace_ptr == 0 { return; }
    if let Some(f) = *ASPACE_CPU_LEAVE.lock() { f(aspace_ptr, cpu); }
}
