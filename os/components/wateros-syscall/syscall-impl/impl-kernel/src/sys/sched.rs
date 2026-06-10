//! `sched_getaffinity(2)` — 单核 CPU 亲和性 stub。

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::TaskId;

use crate::user_copy::copy_to_user;

/// lp64 下单核 stub 返回的有效 mask 字节数（与 Linux 64 位常见行为一致）。
const CPU_MASK_RET_BYTES: usize = 8;

fn resolve_affinity_pid(pid: isize) -> Result<(), ErrNo> {
    if pid == 0 {
        return Ok(());
    }
    if pid < 0 {
        return Err(ErrNo::EINVAL);
    }
    if task::task_snapshot(pid as TaskId).is_some() {
        Ok(())
    } else {
        Err(ErrNo::ESRCH)
    }
}

/// `sched_getaffinity(pid, cpusetsize, mask)` — 单核 stub。
pub(crate) fn sys_sched_getaffinity(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let cpusetsize = args.arg(1);
    let mask_ptr = args.arg(2);

    if mask_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if cpusetsize < CPU_MASK_RET_BYTES {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if let Err(e) = resolve_affinity_pid(pid) {
        return UserRet::from_error(e);
    }

    let mut buf = Vec::with_capacity(cpusetsize);
    buf.resize(cpusetsize, 0);
    buf[0] |= 1;
    match copy_to_user(mask_ptr, &buf) {
        Ok(n) if n == buf.len() => UserRet::from_success(CPU_MASK_RET_BYTES),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}
