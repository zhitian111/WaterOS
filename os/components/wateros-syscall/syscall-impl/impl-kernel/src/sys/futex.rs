//! `futex(2)` — 用户态快速互斥锁原语；等待/唤醒委托 [`ipc::futex::FutexHub`]。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::futex::{FutexKey, FutexHub, FutexWaitOutcome, KernelFutexOps};
use task::TaskTick;

use crate::user_copy::{copy_from_user, copy_from_user_struct};

use super::robust::futex_error_to_errno;

// ── futex 操作码 ──────────────────────────────────────────────────

const FUTEX_WAIT: u32 = 0;
const FUTEX_WAKE: u32 = 1;
const FUTEX_WAIT_BITSET: u32 = 9;
const FUTEX_WAKE_BITSET: u32 = 10;
const FUTEX_REQUEUE: u32 = 3;

const FUTEX_CMD_MASK: u32 = !(ipc::futex::FUTEX_PRIVATE_FLAG);

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec: isize,
    nsec: isize,
}

fn read_user_u32(uaddr: usize) -> Result<u32, ErrNo> {
    let mut val: u32 = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut((&raw mut val) as *mut u8, 4) };
    if copy_from_user(buf, uaddr)? != 4 {
        return Err(ErrNo::EFAULT);
    }
    Ok(val)
}

fn parse_futex_timeout(timeout_ptr: usize) -> Result<Option<TaskTick>, ErrNo> {
    if timeout_ptr == 0 {
        return Ok(None);
    }
    let ts = copy_from_user_struct::<UserTimespec>(timeout_ptr)?;
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    if ts.sec == 0 && ts.nsec == 0 {
        return Ok(Some(0));
    }
    // 与 `nanosleep` 一致：非零超时暂映射为 1 tick。
    Ok(Some(1))
}

fn require_private_key(futex_op: u32, uaddr: usize) -> Result<FutexKey, ErrNo> {
    let key = FutexKey::from_syscall(uaddr, futex_op);
    if !key.is_private {
        return Err(ErrNo::EINVAL);
    }
    Ok(key)
}

fn futex_wait(
    uaddr: usize,
    val: u32,
    bitset: u32,
    futex_op: u32,
    timeout_ptr: usize,
) -> Result<usize, ErrNo> {
    let _ = bitset;

    let cur = read_user_u32(uaddr)?;
    if cur != val {
        return Err(ErrNo::EAGAIN);
    }

    let key = require_private_key(futex_op, uaddr)?;
    let timeout = parse_futex_timeout(timeout_ptr)?;
    if timeout == Some(0) {
        return Err(ErrNo::ETIMEDOUT);
    }

    let hub = FutexHub::global();
    hub.wait_while(key, timeout, || read_user_u32(uaddr).map_or(false, |v| v == val))
        .into_result()
        .map_err(futex_error_to_errno)?;
    Ok(0)
}

fn futex_wake(uaddr: usize, max_wake: u32, bitset: u32, futex_op: u32) -> Result<usize, ErrNo> {
    let _ = bitset;
    let key = require_private_key(futex_op, uaddr)?;
    FutexHub::global()
        .wake(key, max_wake)
        .map_err(futex_error_to_errno)
}

pub(crate) fn wake_user_addr(uaddr: usize) -> usize {
    let key = FutexKey::from_uaddr(uaddr);
    FutexHub::global()
        .wake_all(key)
        .unwrap_or(0)
}

pub(crate) fn sys_futex(args: SyscallArgs) -> UserRet {
    let futex_op = args.arg(1) as u32;
    let uaddr = args.arg(0);
    let val = args.arg(2) as u32;
    let timeout_ptr = args.arg(3);
    let val3 = args.arg(5) as u32;

    let cmd = futex_op & FUTEX_CMD_MASK;

    let result = match cmd {
        FUTEX_WAIT => futex_wait(uaddr, val, 0, futex_op, timeout_ptr),
        FUTEX_WAIT_BITSET => futex_wait(uaddr, val, val3, futex_op, timeout_ptr),
        FUTEX_WAKE => futex_wake(uaddr, val, 0, futex_op),
        FUTEX_WAKE_BITSET => futex_wake(uaddr, val, val3, futex_op),
        FUTEX_REQUEUE => Err(ErrNo::ENOSYS),
        _ => Err(ErrNo::ENOSYS),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}
