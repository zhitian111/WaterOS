//! `futex(2)` — 用户态快速互斥锁原语；等待/唤醒委托 [`ipc::futex::FutexHub`]。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::futex::{FutexHub, FutexKey, FutexWaitOutcome, KernelFutexOps};
use platform::wall_clock;
use task::TaskTick;

use crate::poll_engine::ns_duration_to_ticks;
use crate::user_copy::{copy_from_user, copy_from_user_struct};

use super::super::task::robust::futex_error_to_errno;

// ── futex 操作码 ──────────────────────────────────────────────────

const FUTEX_WAIT : u32 = 0;
const FUTEX_WAKE : u32 = 1;
const FUTEX_WAIT_BITSET : u32 = 9;
const FUTEX_WAKE_BITSET : u32 = 10;
const FUTEX_REQUEUE : u32 = 3;
const FUTEX_CMP_REQUEUE : u32 = 4;
const FUTEX_CLOCK_REALTIME : u32 = 256;

const FUTEX_CMD_MASK : u32 = !(ipc::futex::FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);
const FUTEX_BITSET_MATCH_ALL : u32 = !0;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

fn read_user_u32(uaddr : usize) -> Result<u32, ErrNo> {
    let mut val : u32 = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut((&raw mut val) as *mut u8, 4) };
    if copy_from_user(buf, uaddr)? != 4 {
        return Err(ErrNo::EFAULT);
    }
    Ok(val)
}

fn timespec_to_ns(ts : UserTimespec) -> Result<u128, ErrNo> {
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    Ok((ts.sec as u128) * 1_000_000_000 + ts.nsec as u128)
}

fn parse_futex_timeout(timeout_ptr : usize, futex_op : u32) -> Result<Option<TaskTick>, ErrNo> {
    if timeout_ptr == 0 {
        return Ok(None);
    }
    let ts = copy_from_user_struct::<UserTimespec>(timeout_ptr)?;
    if ts.sec == 0 && ts.nsec == 0 {
        return Ok(Some(0));
    }
    let ticks = if futex_op & FUTEX_CLOCK_REALTIME != 0 {
        let target_ns = timespec_to_ns(ts)?;
        let now_ns = wall_clock::realtime_ns().map_err(|_| ErrNo::EIO)?;
        if target_ns <= now_ns {
            0
        } else {
            ns_duration_to_ticks(target_ns - now_ns)
        }
    } else {
        ns_duration_to_ticks(timespec_to_ns(ts)?)
    };
    Ok(Some(ticks))
}

fn reject_unsupported_futex_bitset(cmd : u32, bitset : u32) -> Result<(), ErrNo> {
    if cmd != FUTEX_WAIT_BITSET && cmd != FUTEX_WAKE_BITSET {
        return Ok(());
    }
    if bitset == FUTEX_BITSET_MATCH_ALL {
        return Ok(());
    }
    log::warn!(
        "[syscall] futex(nr=98) op={cmd} unsupported bitset={bitset:#x} (only !0 implemented)",
    );
    Err(ErrNo::ENOSYS)
}

fn wake_with_alternate_keys(hub : &FutexHub, key : FutexKey, max_wake : u32) -> Result<usize, ErrNo> {
    let n = hub.wake(key, max_wake).map_err(futex_error_to_errno)?;
    if n > 0 {
        return Ok(n);
    }
    let alt = FutexKey {
        uaddr : key.uaddr,
        is_private : !key.is_private,
    };
    hub.wake(alt, max_wake).map_err(futex_error_to_errno)
}

fn futex_wait(uaddr : usize,
              val : u32,
              bitset : u32,
              futex_op : u32,
              timeout_ptr : usize)
              -> Result<usize, ErrNo> {
    let _ = bitset;

    let key = FutexKey::from_syscall(uaddr, futex_op);
    let is_private = key.is_private;
    let timeout = parse_futex_timeout(timeout_ptr, futex_op)?;
    if timeout == Some(0) {
        let cur = read_user_u32(uaddr)?;
        log::trace!(
            "[pthread-debug] futex_wait zero-timeout uaddr={:#x} op={:#x} val={val} cur={cur} private={is_private}",
            uaddr,
            futex_op,
        );
        if cur != val {
            return Err(ErrNo::EAGAIN);
        }
        return Err(ErrNo::ETIMEDOUT);
    }

    let hub = FutexHub::global();
    loop {
        let cur = read_user_u32(uaddr)?;
        if cur != val {
            log::trace!(
                "[pthread-debug] futex_wait EAGAIN uaddr={:#x} val={val} cur={cur} private={is_private}",
                uaddr,
            );
            super::super::task::bringup_stats::record_futex_wait_eagain();
            return Err(ErrNo::EAGAIN);
        }

        log::trace!(
            "[pthread-debug] futex_wait sleep uaddr={:#x} op={:#x} val={val} cur={cur} private={is_private}",
            uaddr,
            futex_op,
        );
        super::super::task::bringup_stats::record_futex_wait_sleep();

        let outcome = hub.wait_while(key, timeout, || read_user_u32(uaddr).map_or(false, |v| v == val));
        match outcome {
            FutexWaitOutcome::Interrupted => {
                log::trace!("[pthread-debug] futex_wait EINTR uaddr={:#x}", uaddr);
                return Err(ErrNo::EINTR);
            }
            FutexWaitOutcome::TimedOut => {
                log::trace!("[pthread-debug] futex_wait ETIMEDOUT uaddr={:#x}", uaddr);
                return Err(ErrNo::ETIMEDOUT);
            }
            FutexWaitOutcome::Woken => {
                log::trace!("[pthread-debug] futex_wait ok uaddr={:#x} val={val}", uaddr);
                return Ok(0);
            }
        }
    }
}

fn futex_wake(uaddr : usize, max_wake : u32, bitset : u32, futex_op : u32) -> Result<usize, ErrNo> {
    let _ = bitset;
    let key = FutexKey::from_syscall(uaddr, futex_op);
    wake_with_alternate_keys(FutexHub::global(), key, max_wake)
}

fn futex_requeue(uaddr : usize,
                 wake_count : u32,
                 requeue_count : u32,
                 uaddr2 : usize,
                 futex_op : u32)
                 -> Result<usize, ErrNo> {
    let from_key = FutexKey::from_syscall(uaddr, futex_op);
    let to_key = FutexKey::from_syscall(uaddr2, futex_op);
    FutexHub::global()
        .requeue(from_key, to_key, wake_count, requeue_count)
        .map_err(futex_error_to_errno)
}

pub(crate) fn wake_user_addr(uaddr : usize) -> usize {
    super::super::task::bringup_stats::record_futex_wake_user_addr();
    // clear_child_tid 的 wake 需要同时尝试 private 和 shared 两种 key，
    // 因为等待者可能用任一种 flag（glibc 可能用 FUTEX_WAIT_BITSET 不带 PRIVATE 标志）
    let hub = FutexHub::global();
    let n1 = hub.wake_all(FutexKey { uaddr,
                                     is_private : true })
                .unwrap_or(0);
    let n2 = hub.wake_all(FutexKey { uaddr,
                                     is_private : false })
                .unwrap_or(0);
    let total = n1 + n2;
    log::trace!(
        "[pthread-debug] wake_user_addr uaddr={:#x} private={n1} shared={n2} total={total}",
        uaddr,
    );
    if total == 0 {
        super::super::task::bringup_stats::record_futex_wake_zero_waiters();
        log::trace!("[pthread-debug] wake_user_addr uaddr={:#x} no waiters", uaddr);
    }
    total
}

// 本方法代码由AI完成
pub(crate) fn sys_futex(args : SyscallArgs) -> UserRet {
    let futex_op = args.arg(1) as u32;
    let uaddr = args.arg(0);
    let val = args.arg(2) as u32;
    let timeout_ptr = args.arg(3);
    let uaddr2 = args.arg(4);
    let val3 = args.arg(5) as u32;

    let cmd = futex_op & FUTEX_CMD_MASK;

    let result = match cmd {
        FUTEX_WAIT => futex_wait(uaddr, val, 0, futex_op, timeout_ptr),
        FUTEX_WAIT_BITSET => match reject_unsupported_futex_bitset(cmd, val3) {
            Ok(()) => futex_wait(uaddr, val, val3, futex_op, timeout_ptr),
            Err(e) => Err(e),
        },
        FUTEX_WAKE => futex_wake(uaddr, val, 0, futex_op),
        FUTEX_WAKE_BITSET => match reject_unsupported_futex_bitset(cmd, val3) {
            Ok(()) => futex_wake(uaddr, val, val3, futex_op),
            Err(e) => Err(e),
        },
        FUTEX_REQUEUE => futex_requeue(uaddr,
                                       val,
                                       timeout_ptr as u32,
                                       uaddr2,
                                       futex_op),
        FUTEX_CMP_REQUEUE => match read_user_u32(uaddr) {
            Ok(cur) if cur == val3 => futex_requeue(uaddr,
                                                    val,
                                                    timeout_ptr as u32,
                                                    uaddr2,
                                                    futex_op),
            Ok(_) => Err(ErrNo::EAGAIN),
            Err(e) => Err(e),
        },
        _ => Err(ErrNo::ENOSYS),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}
