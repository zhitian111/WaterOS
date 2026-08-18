//! 用户地址空间在任务退出时的释放钩子（由 mm-impl 注册，task 在 `exit` 时调用）。

/// 释放 `user_aspace_ptr` 指向的用户页表与映射帧；指针只在内核内部有效，回调实现负责其具体类型转换。
pub type DropUserAspaceFn = fn(usize);
/// 调度器在 CPU 开始或停止使用地址空间时调用的通知钩子，用于维护 SMP 活跃 CPU 集。
pub type AspaceCpuHook = fn(usize, base::cpu::CpuId);

/// 地址空间析构回调；注册只应发生在单线程 MM 初始化阶段，运行期重复注册会替换旧实现。
static DROP_USER_ASPACE: spin::Mutex<Option<DropUserAspaceFn>> = spin::Mutex::new(None);
/// 进入地址空间的 CPU 记账回调，与离开回调必须由同一 MM 实现成对注册。
static ASPACE_CPU_ENTER: spin::Mutex<Option<AspaceCpuHook>> = spin::Mutex::new(None);
/// 离开地址空间的 CPU 记账回调；缺失回调时 dummy/单核实现保持无副作用。
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

/// 注册调度器用于追踪当前正在使用某地址空间的 CPU 的回调。
/// dummy 或单核 MM 可以不注册；调用期间不得持有会被回调路径再次获取的 MM 锁。
pub fn register_aspace_cpu_hooks(enter: AspaceCpuHook, leave: AspaceCpuHook) {
    *ASPACE_CPU_ENTER.lock() = Some(enter);
    *ASPACE_CPU_LEAVE.lock() = Some(leave);
}

/// 通知指定 CPU 即将使用该地址空间；零指针表示内核地址空间，不触发用户页表记账。
pub fn notify_aspace_cpu_enter(aspace_ptr: usize, cpu: base::cpu::CpuId) {
    if aspace_ptr == 0 { return; }
    if let Some(f) = *ASPACE_CPU_ENTER.lock() { f(aspace_ptr, cpu); }
}

/// 通知指定 CPU 不再使用该地址空间；必须与 enter 在调度/切换边界成对出现以保证 shootdown 正确。
pub fn notify_aspace_cpu_leave(aspace_ptr: usize, cpu: base::cpu::CpuId) {
    if aspace_ptr == 0 { return; }
    if let Some(f) = *ASPACE_CPU_LEAVE.lock() { f(aspace_ptr, cpu); }
}
