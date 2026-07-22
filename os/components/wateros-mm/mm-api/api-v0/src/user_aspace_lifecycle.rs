//! 用户地址空间在任务退出时的释放钩子（由 mm-impl 注册，task 在 `exit` 时调用）。

/// 释放 `user_aspace_ptr` 指向的用户页表与映射帧。
pub type DropUserAspaceFn = fn(usize);

static DROP_USER_ASPACE: spin::Mutex<Option<DropUserAspaceFn>> = spin::Mutex::new(None);

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
