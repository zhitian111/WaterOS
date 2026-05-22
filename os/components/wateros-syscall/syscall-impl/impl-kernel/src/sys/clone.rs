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
    if child_stack == 0 {
        if let Some(snap) = task::current_task_snapshot() {
            if let Some(ur) = snap.user_resources {
                let size = ur.user_stack_size;
                if size != 0 && ur.user_aspace_ptr != 0 {
                    let parent_sp = snap.trap_frame
                                        .map_or(ur.user_stack_bottom, |t| t.user_sp);
                    let official_bottom = ur.user_stack_bottom;
                    let stack_top = ur.user_stack_top;
                    let eff_bottom = core::cmp::min(official_bottom, parent_sp) & !0xFFF;
                    let eff_size = stack_top.saturating_sub(eff_bottom);
                    // 子栈紧贴有效区下方，与父栈 VA 不重叠
                    let child_top = eff_bottom;
                    let child_bottom = eff_bottom.saturating_sub(eff_size);

                    // 父进程 sp 溢出官方栈底——先把缺失页补上，父子都能用
                    if eff_bottom < official_bottom {
                        with_user_aspace_mut(ur.user_aspace_ptr, |aspace| {
                            let mut vpn = VirtAddr(eff_bottom).floor_page();
                            let vpn_end = VirtAddr(official_bottom).ceil_page();
                            while vpn.0 < vpn_end.0 {
                                let ppn = frame_alloc_result().expect("fork: extend frame");
                                match aspace.map_page_to_ppn(vpn,
                                                             ppn,
                                                             PagePerm::R |
                                                             PagePerm::W |
                                                             PagePerm::U)
                                {
                                    Ok(()) | Err(mm::api::error::MmError::AlreadyMapped) => {}
                                    Err(e) => return Err(e),
                                }
                                vpn = VirtPageNum(vpn.0 + 1);
                            }
                            Ok(())
                        }).expect("fork: extend parent stack");
                    }

                    with_user_aspace_mut(ur.user_aspace_ptr, |aspace| {
                        let mut vpn = VirtAddr(child_bottom).floor_page();
                        let vpn_end = VirtAddr(child_top).ceil_page();
                        while vpn.0 < vpn_end.0 {
                            if aspace.translate_addr(vpn.start_addr())?
                                     .is_none()
                            {
                                let ppn : PhysPageNum =
                                    frame_alloc_result().expect("fork: child frame alloc");
                                aspace.map_page_to_ppn(vpn,
                                                       ppn,
                                                       PagePerm::R | PagePerm::W | PagePerm::U)?;
                            }
                            vpn = VirtPageNum(vpn.0 + 1);
                        }
                        Ok(())
                    }).expect("fork: child stack map");

                    // 新建的映射须冲刷 TLB
                    unsafe {
                        core::arch::asm!("sfence.vma x0, x0");
                    }

                    let ops = Sv39UserMemoryOps::new(ur.user_aspace_ptr);
                    let mut buf = Vec::new();
                    buf.resize(eff_size, 0);
                    ops.copy_from_user(&mut buf, VirtAddr(eff_bottom))
                       .expect("fork: read parent stack");
                    remap_stack_refs(&mut buf,
                                     eff_bottom,
                                     stack_top,
                                     child_bottom);
                    ops.copy_to_user(VirtAddr(child_bottom), &buf)
                       .expect("fork: write child stack");

                    task::prepare_fork_user_stack_range(child_bottom, child_top);
                }
            }
        }
    }
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
