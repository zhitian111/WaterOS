//! `get_mempolicy(2)` — 单节点 NUMA stub（非 NUMA 机器兼容路径）。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use mm::api::addr::VirtAddr;
use mm::api::address_space::AddressSpaceOps;

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};
use crate::user_copy::{copy_to_user, copy_to_user_struct};

const MPOL_DEFAULT: i32 = 0;
const MPOL_F_NODE: usize = 1;
const MPOL_F_ADDR: usize = 2;
const MPOL_F_MEMS_ALLOWED: usize = 4;
const MPOL_VALID_FLAGS: usize = MPOL_F_NODE | MPOL_F_ADDR | MPOL_F_MEMS_ALLOWED;

fn verify_user_addr_mapped(addr: usize) -> Result<(), ErrNo> {
    if addr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let Some(handle) = current_user_aspace_handle() else {
        return Err(ErrNo::EFAULT);
    };
    mm::user_aspace::with_user_aspace_mut(handle, |aspace| {
        aspace.translate_addr(VirtAddr(addr))
    })
    .map_err(mm_err_to_errno)
    .and_then(|opt| {
        if opt.is_some() {
            Ok(())
        } else {
            Err(ErrNo::EFAULT)
        }
    })
}

fn write_nodemask_node0(nodemask_ptr: usize, maxnode: usize) -> Result<(), ErrNo> {
    if nodemask_ptr == 0 {
        return Ok(());
    }
    if maxnode == 0 {
        return Err(ErrNo::EINVAL);
    }
    let nbytes = (maxnode + 7) / 8;
    let mut buf = Vec::with_capacity(nbytes);
    buf.resize(nbytes, 0);
    buf[0] |= 1;
    match copy_to_user(nodemask_ptr, &buf) {
        Ok(n) if n == buf.len() => Ok(()),
        Ok(_) => Err(ErrNo::EFAULT),
        Err(e) => Err(e),
    }
}

/// `get_mempolicy(mode, nodemask, maxnode, addr, flags)` — 单节点 stub。
pub(crate) fn sys_get_mempolicy(args: SyscallArgs) -> UserRet {
    let mode_ptr = args.arg(0);
    let nodemask_ptr = args.arg(1);
    let maxnode = args.arg(2);
    let addr = args.arg(3);
    let flags = args.arg(4);

    if flags & !MPOL_VALID_FLAGS != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let node_flag = flags & MPOL_F_NODE != 0;
    let addr_flag = flags & MPOL_F_ADDR != 0;

    if addr_flag {
        if let Err(e) = verify_user_addr_mapped(addr) {
            return UserRet::from_error(e);
        }
    }

    if !node_flag {
        if mode_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        if let Err(e) = copy_to_user_struct(mode_ptr, &MPOL_DEFAULT) {
            return UserRet::from_error(e);
        }
    }

    if let Err(e) = write_nodemask_node0(nodemask_ptr, maxnode) {
        return UserRet::from_error(e);
    }

    UserRet::from_success(0)
}
