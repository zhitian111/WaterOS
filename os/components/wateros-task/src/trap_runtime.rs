//! Trap 与任务运行时的 **Rust 具名入口**：供组合层 trap handler 在进入/返回
//! trap 时访问当前任务现场，并在返回前激活正确的地址空间。
//!
//! 与任务首次入口相关的 C ABI 符号集中在私有 `entry_runtime` 模块；普通
//! trap/syscall/interrupt 路径直接调用本模块的 Rust 函数。

use crate::scheduler;
use crate::scheduler::TaskTrapFrame;
use arch::trap::TrapAddressSpaceWrite;
use core::sync::atomic::{AtomicUsize, Ordering};

// 由 `init_kernel_address_space_token` 在内核地址空间就绪后写入；
// `restore_current_trap_frame` 在返回路径读回并写入 trap frame。
// 使用 `Release`/`Relaxed` 与单次初始化路径匹配。
static KERNEL_ADDRESS_SPACE_TOKEN: AtomicUsize = AtomicUsize::new(0);

/// 在全局内核地址空间 token 就绪后注册，供 trap 返回内核态时恢复。
#[inline]
pub(crate) fn init_kernel_address_space_token(token: usize) {
    KERNEL_ADDRESS_SPACE_TOKEN.store(token, Ordering::Release);
}

fn return_address_space_token(returns_to_user: bool) -> usize {
    let kernel_token = KERNEL_ADDRESS_SPACE_TOKEN.load(Ordering::Relaxed);
    if !returns_to_user {
        return kernel_token;
    }
    let raw = scheduler::current_task_address_space_raw();
    if raw == 0 {
        kernel_token
    } else {
        raw
    }
}

/// 当前任务返回用户态时应使用的地址空间 token。
#[inline]
pub(crate) fn current_user_return_address_space_token() -> usize {
    return_address_space_token(true)
}

/// 解析 trap 帧归属任务，返回应被 Rust 侧修改的权威 `TrapContext` 指针。
#[inline]
pub(crate) unsafe fn begin_current_trap_frame_access(trap_frame_ptr: *mut u8) -> *mut u8 {
    let trap_frame = unsafe { *(trap_frame_ptr as *const TaskTrapFrame) };
    scheduler::begin_current_trap_frame_access(trap_frame)
        .map(|p| p.cast::<u8>())
        .unwrap_or(trap_frame_ptr)
}

/// 将当前任务保存区内的权威 trap 帧写回栈上 trap 帧，并写入返回地址空间 token。
#[inline]
pub(crate) unsafe fn restore_current_trap_frame(trap_frame_ptr: *mut u8) -> bool {
    let trap_frame = unsafe { &mut *(trap_frame_ptr as *mut TaskTrapFrame) };
    let restored = scheduler::restore_current_trap_frame(trap_frame);
    let token = return_address_space_token(
        <TaskTrapFrame as arch::trap::TrapFrameRead>::returns_to_user(trap_frame),
    );
    trap_frame.prepare_address_space_for_return(token);
    restored
}
