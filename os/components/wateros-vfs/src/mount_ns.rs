//! per-task 挂载命名空间（`impl-fd-session` + `bridge-fs-api`）。

#![cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]

/// 新任务初始化独立挂载命名空间。
pub fn init_task_mount_ns(task_id: task::TaskId) {
    impl_fs_bridge::init_task_mount_ns(task_id);
}

/// 首个用户任务继承启动它的内核任务 namespace；无当前任务时继承 bootstrap。
pub fn on_user_task_spawned(task_id: task::TaskId) {
    // 有当前任务时继承其 namespace；启动早期没有父任务则建立 bootstrap namespace。
    if let Some(parent_id) = task::current_task_id() {
        copy_mount_ns_from_parent(task_id, parent_id);
    } else {
        init_task_mount_ns(task_id);
    }
}

/// fork 时复制父任务挂载命名空间。
pub fn copy_mount_ns_from_parent(child: task::TaskId, parent: task::TaskId) {
    impl_fs_bridge::copy_mount_ns_from_parent(child, parent);
}

/// thread clone 时共享父任务挂载命名空间。
pub fn share_mount_ns_from_parent(child: task::TaskId, parent: task::TaskId) {
    impl_fs_bridge::share_mount_ns_from_parent(child, parent);
}

/// `CLONE_NEWNS` / `unshare(CLONE_NEWNS)`：为任务创建独立挂载命名空间。
pub fn unshare_mount_ns(task_id: task::TaskId) {
    impl_fs_bridge::unshare_mount_ns(task_id);
}

/// 任务退出后释放挂载命名空间槽位。
pub fn drop_task_mount_ns(task_id: task::TaskId) {
    impl_fs_bridge::drop_task_mount_ns(task_id);
}

pub use impl_fs_bridge::MountPropagation;
