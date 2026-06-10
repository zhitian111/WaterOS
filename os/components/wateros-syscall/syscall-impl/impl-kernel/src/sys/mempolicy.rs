//! `get_mempolicy(2)` — 用户拷贝与参数解析；语义委托 [`mm::mempolicy`]。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use mm::api::mempolicy::{MempolicyError, MPOL_F_ADDR, MPOL_F_NODE};

use crate::mm_util::{current_user_aspace_handle, mm_err_to_errno};
use crate::user_copy::{copy_to_user, copy_to_user_struct};

fn mempolicy_err_to_errno(err: MempolicyError) -> ErrNo {
    match err {
        MempolicyError::InvalidArg => ErrNo::EINVAL,
    }
}

/// `get_mempolicy(mode, nodemask, maxnode, addr, flags)`。
pub(crate) fn sys_get_mempolicy(args: SyscallArgs) -> UserRet {
    let mode_ptr = args.arg(0);
    let nodemask_ptr = args.arg(1);
    let maxnode = args.arg(2);
    let addr = args.arg(3);
    let flags = args.arg(4);

    let node_flag = flags & MPOL_F_NODE != 0;
    let addr_flag = flags & MPOL_F_ADDR != 0;
    let write_nodemask = nodemask_ptr != 0;

    if addr_flag {
        if addr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        let Some(handle) = current_user_aspace_handle() else {
            return UserRet::from_error(ErrNo::EFAULT);
        };
        match mm::mempolicy::is_user_addr_mapped(handle, addr) {
            Ok(true) => {}
            Ok(false) => return UserRet::from_error(ErrNo::EFAULT),
            Err(e) => return UserRet::from_error(mm_err_to_errno(e)),
        }
    }

    if !node_flag {
        if mode_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }

    let result = match mm::mempolicy::get_mempolicy_single_node(flags, maxnode, write_nodemask)
    {
        Ok(value) => value,
        Err(e) => return UserRet::from_error(mempolicy_err_to_errno(e)),
    };

    if !node_flag {
        if let Err(e) = copy_to_user_struct(mode_ptr, &result.mode) {
            return UserRet::from_error(e);
        }
    }

    if write_nodemask {
        let mut buf = Vec::with_capacity(result.nodemask_len);
        buf.resize(result.nodemask_len, 0);
        mm::mempolicy::fill_get_mempolicy_nodemask(&mut buf);
        match copy_to_user(nodemask_ptr, &buf) {
            Ok(n) if n == buf.len() => {}
            Ok(_) => return UserRet::from_error(ErrNo::EFAULT),
            Err(e) => return UserRet::from_error(e),
        }
    }

    UserRet::from_success(0)
}
