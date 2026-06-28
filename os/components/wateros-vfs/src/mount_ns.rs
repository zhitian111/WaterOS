//! per-task 挂载命名空间（`impl-fd-session` + `bridge-fs-api`）。

#![cfg(all(feature = "impl-fd-session", feature = "bridge-fs-api"))]

/// 新任务初始化独立挂载命名空间。
pub fn init_task_mount_ns(task_id: task::TaskId) {
    impl_fs_bridge::init_task_mount_ns(task_id);
}

pub fn on_user_task_spawned(task_id: task::TaskId) {
    init_task_mount_ns(task_id);
}

pub fn copy_mount_ns_from_parent(child: task::TaskId, parent: task::TaskId) {
    impl_fs_bridge::copy_mount_ns_from_parent(child, parent);
}

pub fn share_mount_ns_from_parent(child: task::TaskId, parent: task::TaskId) {
    impl_fs_bridge::share_mount_ns_from_parent(child, parent);
}

pub fn unshare_mount_ns(task_id: task::TaskId) {
    impl_fs_bridge::unshare_mount_ns(task_id);
}

pub fn drop_task_mount_ns(task_id: task::TaskId) {
    impl_fs_bridge::drop_task_mount_ns(task_id);
}

pub use impl_fs_bridge::MountPropagation;
