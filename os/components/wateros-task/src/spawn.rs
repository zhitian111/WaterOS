//! 任务创建接口。

use mm_api::kernel_bringup::LoadedElf;

use crate::{
    active_impl, scheduler, AddressSpaceHandle, AddressSpaceRef, KernelTaskEntry, TaskId,
    UserImageInfo, UserStack, UserTask,
};

/// 创建一个新的内核任务，传入函数入口和参数。
pub fn spawn_kernel_task(entry : KernelTaskEntry, arg : usize) -> TaskId {
    scheduler::spawn_kernel_task(entry, arg)
}

/// 按给定规格创建一个尚未进入就绪队列的用户任务。
///
/// 调用方必须先初始化 credential、cwd、mount namespace 等 per-task 侧表，再调用
/// [`start_user_task`]。这与 fork/clone 的两阶段发布协议一致。
pub fn create_user_task(user : UserTask) -> TaskId {
    let task_id = scheduler::create_user_task_spec(user);
    let parent_pid = crate::process::current_process_task_snapshot().map(|task| task.pid);
    let address_space = user_address_space_ref(user);
    active_impl::with_process_registry(|registry| {
        registry.create_process_for_task(task_id, parent_pid, address_space)
    }).expect("new user task must have a fresh process-registry entry");
    task_id
}

/// 完成 per-task 侧表初始化后，将新用户任务发布到调度器。
pub fn start_user_task(task_id : TaskId) { scheduler::enqueue_ready_task(task_id); }

/// 按给定规格创建并立即发布用户任务。
///
/// 仅适用于不需要在首次运行前初始化额外侧表的调用方。bring-up 等复杂创建路径应显式
/// 使用 [`create_user_task`] 和 [`start_user_task`]。
pub fn spawn_user_task(user : UserTask) -> TaskId {
    let task_id = create_user_task(user);
    start_user_task(task_id);
    task_id
}

/// 基于 MM 已装载的 ELF 创建一个用户任务，并返回分配到的任务号。
pub fn spawn_user_task_from_loaded_elf(loaded : &LoadedElf) -> TaskId {
    spawn_user_task(user_task_from_loaded_elf(loaded))
}

/// 将 MM ELF loader 产出的地址空间、映像与外部用户栈元数据转换为用户任务规格。
pub fn user_task_from_loaded_elf(loaded : &LoadedElf) -> UserTask {
    UserTask::new(loaded.entry_pc,
                  AddressSpaceHandle::from_raw(loaded.satp),
                  UserImageInfo::new(loaded.image_base, loaded.image_size),
                  UserStack::from_range(loaded.stack_bottom, loaded.stack_top),
                  loaded.user_aspace_ptr)
}

fn user_address_space_ref(user : UserTask) -> Option<AddressSpaceRef> {
    Some(AddressSpaceRef::new(user.address_space()?,
                              user.user_aspace_ptr()?))
}
