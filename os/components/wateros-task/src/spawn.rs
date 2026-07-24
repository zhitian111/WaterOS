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

/// 按给定规格创建一个新的用户任务，并返回分配到的任务号。
pub fn spawn_user_task(user : UserTask) -> TaskId {
    let task_id = scheduler::create_user_task_spec(user);
    let parent_pid = crate::process::current_process_task_snapshot().map(|task| task.pid);
    let address_space = user_address_space_ref(user);
    active_impl::with_process_registry(|registry| {
        registry.create_process_for_task(task_id, parent_pid, address_space)
    }).expect("new user task must have a fresh process-registry entry");
    scheduler::enqueue_ready_task(task_id);
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
