//! `clone`/`fork` 系统调用实现。
//!
//! 当前仅支持最小 fork 语义（`clone` 不带 `CLONE_VM`/`CLONE_THREAD` 等标志）：
//! 创建一个子任务，共享父任务地址空间与用户栈区间，子任务获得父任务 trap 帧副本
//! （a0 置 0），并继承父任务的 cwd。
//!
//! fork（`child_stack=0`）时子任务获得用户栈底部区域的独立 SP，
//! 避免子进程在共享栈上写数据破坏父进程栈帧。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use alloc::vec::Vec;
use mm::{
    api::user_access::UserMemoryOps,
    api::{
        addr::{PhysPageNum, VirtAddr, VirtPageNum},
        address_space::AddressSpaceOps,
        perm::PagePerm,
    },
    frame_alloctor::frame_alloc_result,
    user_access::Sv39UserMemoryOps,
    user_aspace::with_user_aspace_mut,
};

/// clone/fork 系统调用入口。
///
/// 参数（Linux riscv64 clone ABI）：
/// - `arg0`: flags（当前仅忽略）
/// - `arg1`: child_stack（0 表示复用父任务栈指针）
/// - 其余参数暂未处理
pub(crate) fn sys_clone(args : SyscallArgs) -> UserRet {
    let child_stack = args.arg(1);
    do_clone(child_stack)
}

#[inline(never)]
fn do_clone(child_stack : usize) -> UserRet {
    // TODO: 在新 TCB 设计下恢复 fork 路径（user_resources 已从 TaskSnapshot 移除）
    match task::fork_current(child_stack) {
        Some(child_id) => UserRet::from_success(child_id),
        None => UserRet::from_error(ErrNo::EAGAIN),
    }
}

fn remap_stack_refs(buf : &mut [u8], parent_lo : usize, parent_hi : usize, child_lo : usize) {
    for word in buf.chunks_exact_mut(core::mem::size_of::<usize>()) {
        let val = usize::from_ne_bytes(word.try_into()
                                           .unwrap());
        if val >= parent_lo && val < parent_hi {
            let translated = child_lo + (val - parent_lo);
            word.copy_from_slice(&translated.to_ne_bytes());
        }
    }
}
