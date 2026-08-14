//! `futex(2)` — 用户态快速互斥锁原语；等待/唤醒委托 `ipc::futex` facade。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use ipc::futex::{FutexError, FutexKey, FutexWaitOutcome};
use mm::api::user_access::FutexMappingIdentity;
use platform::wall_clock;
use task::TaskTick;

use crate::poll_engine::ns_duration_to_ticks;
use crate::user_copy::{
    atomic_compare_exchange_user_u32_in_aspace, atomic_load_user_u32_in_aspace,
    copy_from_user_struct, futex_mapping_identity_u32_in_aspace,
};

use super::futex_error_to_errno;

// ── futex 操作码 ──────────────────────────────────────────────────

const FUTEX_WAIT : u32 = 0;
const FUTEX_WAKE : u32 = 1;
const FUTEX_WAIT_BITSET : u32 = 9;
const FUTEX_WAKE_BITSET : u32 = 10;
const FUTEX_REQUEUE : u32 = 3;
const FUTEX_CMP_REQUEUE : u32 = 4;
const FUTEX_WAKE_OP : u32 = 5;
const FUTEX_CLOCK_REALTIME : u32 = 256;

const FUTEX_CMD_MASK : u32 = !(ipc::futex::FUTEX_PRIVATE_FLAG | FUTEX_CLOCK_REALTIME);
const FUTEX_BITSET_MATCH_ALL : u32 = !0;

const FUTEX_OP_SET : u32 = 0;
const FUTEX_OP_ADD : u32 = 1;
const FUTEX_OP_OR : u32 = 2;
const FUTEX_OP_ANDN : u32 = 3;
const FUTEX_OP_XOR : u32 = 4;

const FUTEX_OP_CMP_EQ : u32 = 0;
const FUTEX_OP_CMP_NE : u32 = 1;
const FUTEX_OP_CMP_LT : u32 = 2;
const FUTEX_OP_CMP_LE : u32 = 3;
const FUTEX_OP_CMP_GT : u32 = 4;
const FUTEX_OP_CMP_GE : u32 = 5;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[derive(Clone, Copy)]
struct FutexDeadline {
    target_ns : u128,
    realtime : bool,
}

impl FutexDeadline {
    fn now_ns(self) -> Result<u128, ErrNo> {
        if self.realtime {
            wall_clock::realtime_ns()
        } else {
            wall_clock::monotonic_ns()
        }.map_err(|_| ErrNo::EIO)
    }

    fn remaining_ticks(self) -> Result<TaskTick, ErrNo> {
        Ok(ns_duration_to_ticks(self.target_ns
                                    .saturating_sub(self.now_ns()?)))
    }
}

fn read_user_u32_in_aspace(aspace : usize, uaddr : usize) -> Result<u32, ErrNo> {
    atomic_load_user_u32_in_aspace(aspace, uaddr)
}

fn timespec_to_ns(ts : UserTimespec) -> Result<u128, ErrNo> {
    if ts.sec < 0 || ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
        return Err(ErrNo::EINVAL);
    }
    Ok((ts.sec as u128) * 1_000_000_000 + ts.nsec as u128)
}

fn validate_futex_uaddr(uaddr : usize) -> Result<(), ErrNo> {
    if uaddr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if uaddr % core::mem::size_of::<u32>() != 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

fn validate_requeue_count(count : u32) -> Result<u32, ErrNo> {
    if count > i32::MAX as u32 {
        return Err(ErrNo::EINVAL);
    }
    Ok(count)
}

fn parse_futex_timeout(timeout_ptr : usize,
                       cmd : u32,
                       futex_op : u32)
                       -> Result<Option<FutexDeadline>, ErrNo> {
    if timeout_ptr == 0 {
        return Ok(None);
    }
    let ts = copy_from_user_struct::<UserTimespec>(timeout_ptr)?;
    let timeout_ns = timespec_to_ns(ts)?;
    let deadline = match cmd {
        // FUTEX_WAIT 使用相对时长。
        FUTEX_WAIT => {
            let now_ns = wall_clock::monotonic_ns().map_err(|_| ErrNo::EIO)?;
            FutexDeadline { target_ns : now_ns.saturating_add(timeout_ns),
                            realtime : false }
        }
        // FUTEX_WAIT_BITSET 使用绝对 deadline；未指定 CLOCK_REALTIME 时
        // 基于对应时钟，而不是把绝对值再次当作相对时长。
        FUTEX_WAIT_BITSET => FutexDeadline { target_ns : timeout_ns,
                                             realtime : futex_op & FUTEX_CLOCK_REALTIME != 0 },
        _ => return Err(ErrNo::EINVAL),
    };
    Ok(Some(deadline))
}

fn validate_futex_bitset(cmd : u32, bitset : u32) -> Result<(), ErrNo> {
    if cmd != FUTEX_WAIT_BITSET && cmd != FUTEX_WAKE_BITSET {
        return Ok(());
    }
    if bitset == 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

#[inline]
fn current_futex_scope() -> usize { task::current_task_user_aspace_ptr() }

fn futex_key_in_aspace(uaddr : usize,
                       futex_op : u32,
                       user_aspace : usize)
                       -> Result<FutexKey, ErrNo> {
    if futex_op & ipc::futex::FUTEX_PRIVATE_FLAG != 0 {
        Ok(FutexKey::private(uaddr, user_aspace))
    } else {
        match futex_mapping_identity_u32_in_aspace(user_aspace, uaddr)? {
            FutexMappingIdentity::Private => Ok(FutexKey::private(uaddr, user_aspace)),
            FutexMappingIdentity::Shared(identity) => Ok(FutexKey::shared(identity)),
        }
    }
}

pub(crate) fn nonprivate_futex_key_for_aspace(user_aspace : usize,
                                              uaddr : usize)
                                              -> Result<FutexKey, ErrNo> {
    validate_futex_uaddr(uaddr)?;
    futex_key_in_aspace(uaddr, 0, user_aspace)
}

fn futex_wait(uaddr : usize,
              val : u32,
              bitset : u32,
              cmd : u32,
              futex_op : u32,
              timeout_ptr : usize)
              -> Result<usize, ErrNo> {
    let _ = bitset;
    validate_futex_uaddr(uaddr)?;

    // 后续条件复查会在 scheduler 全局锁内执行；必须提前捕获地址空间，
    // 不能在闭包中再次经 task API 查询当前任务而重入 scheduler 锁。
    let current_aspace = current_futex_scope();
    let key = futex_key_in_aspace(uaddr, futex_op, current_aspace)?;
    let is_private = key.is_private;
    let timeout = parse_futex_timeout(timeout_ptr, cmd, futex_op)?;
    if timeout.map(FutexDeadline::remaining_ticks)
              .transpose()? ==
       Some(0)
    {
        let cur = read_user_u32_in_aspace(current_aspace, uaddr)?;
        log::trace!("[pthread-debug] futex_wait zero-timeout uaddr={:#x} op={:#x} val={val} \
                     cur={cur} private={is_private}",
                    uaddr,
                    futex_op,);
        if cur != val {
            return Err(ErrNo::EAGAIN);
        }
        return Err(ErrNo::ETIMEDOUT);
    }

    let cur = read_user_u32_in_aspace(current_aspace, uaddr)?;
    if cur != val {
        log::trace!("[pthread-debug] futex_wait EAGAIN uaddr={:#x} val={val} cur={cur} \
                     private={is_private}",
                    uaddr,);
        super::super::misc::bringup_stats::record_futex_wait_eagain();
        return Err(ErrNo::EAGAIN);
    }

    log::trace!("[pthread-debug] futex_wait sleep uaddr={:#x} op={:#x} val={val} cur={cur} \
                 private={is_private}",
                uaddr,
                futex_op,);
    super::super::misc::bringup_stats::record_futex_wait_sleep();

    let task_id = task::current_task_id().ok_or(ErrNo::ESRCH)?;
    loop {
        let timeout_ticks = timeout.map(FutexDeadline::remaining_ticks)
                                   .transpose()?;
        if timeout_ticks == Some(0) {
            return Err(ErrNo::ETIMEDOUT);
        }
        let mut condition_error = None;
        let outcome = ipc::futex::wait_while(task_id,
                                             key,
                                             bitset,
                                             timeout_ticks,
                                             || match read_user_u32_in_aspace(current_aspace, uaddr)
                                             {
                                                 Ok(value) => value == val,
                                                 Err(error) => {
                                                     condition_error = Some(error);
                                                     false
                                                 }
                                             });
        if let Some(error) = condition_error {
            return Err(error);
        }
        match outcome {
            FutexWaitOutcome::ConditionChanged => {
                super::super::misc::bringup_stats::record_futex_wait_eagain();
                return Err(ErrNo::EAGAIN);
            }
            FutexWaitOutcome::Interrupted => {
                log::trace!("[pthread-debug] futex_wait EINTR uaddr={:#x}",
                            uaddr);
                return Err(ErrNo::EINTR);
            }
            FutexWaitOutcome::TimedOut => {
                if timeout.map(FutexDeadline::remaining_ticks)
                          .transpose()? !=
                   Some(0)
                {
                    continue;
                }
                log::trace!("[pthread-debug] futex_wait ETIMEDOUT uaddr={:#x}",
                            uaddr);
                return Err(ErrNo::ETIMEDOUT);
            }
            FutexWaitOutcome::Woken => {
                log::trace!("[pthread-debug] futex_wait ok uaddr={:#x} val={val}",
                            uaddr);
                return Ok(0);
            }
        }
    }
}

fn futex_wake(uaddr : usize, max_wake : u32, bitset : u32, futex_op : u32) -> Result<usize, ErrNo> {
    validate_futex_uaddr(uaddr)?;
    let key = futex_key_in_aspace(uaddr, futex_op, current_futex_scope())?;
    Ok(if bitset == FUTEX_BITSET_MATCH_ALL {
        ipc::futex::wake(key, max_wake)
    } else {
        ipc::futex::wake_bitset(key, max_wake, bitset)
    })
}

fn futex_requeue(uaddr : usize,
                 wake_count : u32,
                 requeue_count : u32,
                 uaddr2 : usize,
                 futex_op : u32,
                 compare : Option<u32>)
                 -> Result<usize, ErrNo> {
    let wake_count = validate_requeue_count(wake_count)?;
    let requeue_count = validate_requeue_count(requeue_count)?;
    validate_futex_uaddr(uaddr)?;
    validate_futex_uaddr(uaddr2)?;
    let private_scope = current_futex_scope();
    let from_key = futex_key_in_aspace(uaddr, futex_op, private_scope)?;
    let to_key = futex_key_in_aspace(uaddr2, futex_op, private_scope)?;
    if let Some(expected) = compare {
        // 先在 scheduler 锁外触发可能的缺页，再由 cmp_requeue 在队列迁移
        // 临界区内复读，保证比较与迁移具有同一个线性化点。
        if read_user_u32_in_aspace(private_scope, uaddr)? != expected {
            return Err(ErrNo::EAGAIN);
        }
        ipc::futex::cmp_requeue(
                                from_key,
                                to_key,
                                wake_count,
                                requeue_count,
                                || {
                                    read_user_u32_in_aspace(private_scope, uaddr)
                .map(|value| value == expected)
                .map_err(|_| FutexError::Fault)
                                },
        ).map_err(futex_error_to_errno)
    } else {
        ipc::futex::requeue(from_key,
                            to_key,
                            wake_count,
                            requeue_count).map_err(futex_error_to_errno)
    }
}

/// 将编码字段按 Linux futex ABI 解释为有符号 12 位立即数。
#[inline]
fn sign_extend_12(value : u32) -> i32 { ((value << 20) as i32) >> 20 }

fn decode_wake_op(encoded : u32) -> Result<(u32, u32, i32, i32), ErrNo> {
    let op_field = encoded >> 28;
    let op = op_field & 0x7;
    let cmp = (encoded >> 24) & 0xF;
    let mut op_arg = sign_extend_12((encoded >> 12) & 0xFFF);
    let cmp_arg = sign_extend_12(encoded & 0xFFF);
    if op > FUTEX_OP_XOR || cmp > FUTEX_OP_CMP_GE {
        return Err(ErrNo::ENOSYS);
    }
    // FUTEX_OP_OPARG_SHIFT 使参数表示 `1 << op_arg`。Linux 拒绝负数和
    // 会触及符号位的移位，避免 C ABI 中的未定义行为。
    if op_field & 0x8 != 0 {
        if !(0..31).contains(&op_arg) {
            return Err(ErrNo::EINVAL);
        }
        op_arg = 1i32 << op_arg;
    }
    Ok((op, cmp, op_arg, cmp_arg))
}

fn apply_wake_op(old : u32, op : u32, argument : i32) -> u32 {
    let argument = argument as u32;
    match op {
        FUTEX_OP_SET => argument,
        FUTEX_OP_ADD => old.wrapping_add(argument),
        FUTEX_OP_OR => old | argument,
        FUTEX_OP_ANDN => old & !argument,
        FUTEX_OP_XOR => old ^ argument,
        _ => unreachable!("wake op was validated"),
    }
}

fn wake_op_comparison(old : u32, cmp : u32, argument : i32) -> bool {
    let old = old as i32;
    match cmp {
        FUTEX_OP_CMP_EQ => old == argument,
        FUTEX_OP_CMP_NE => old != argument,
        FUTEX_OP_CMP_LT => old < argument,
        FUTEX_OP_CMP_LE => old <= argument,
        FUTEX_OP_CMP_GT => old > argument,
        FUTEX_OP_CMP_GE => old >= argument,
        _ => unreachable!("wake comparison was validated"),
    }
}

/// `FUTEX_WAKE_OP`：原子更新 `uaddr2`，总是唤醒第一队列，并在旧值满足
/// 比较条件时唤醒第二队列。CAS 既保证 SMP 原子性，也允许用户页首次缺页。
fn futex_wake_op(uaddr : usize,
                 wake_count : u32,
                 wake_count2 : u32,
                 uaddr2 : usize,
                 encoded : u32,
                 futex_op : u32)
                 -> Result<usize, ErrNo> {
    let wake_count = validate_requeue_count(wake_count)?;
    let wake_count2 = validate_requeue_count(wake_count2)?;
    validate_futex_uaddr(uaddr)?;
    validate_futex_uaddr(uaddr2)?;
    let (op, cmp, op_arg, cmp_arg) = decode_wake_op(encoded)?;
    let aspace = current_futex_scope();
    // 先解析两个 key，确保任何地址错误发生在用户字被修改之前。
    let key1 = futex_key_in_aspace(uaddr, futex_op, aspace)?;
    let key2 = futex_key_in_aspace(uaddr2, futex_op, aspace)?;

    let old = loop {
        let observed = atomic_load_user_u32_in_aspace(aspace, uaddr2)?;
        let desired = apply_wake_op(observed, op, op_arg);
        let actual = atomic_compare_exchange_user_u32_in_aspace(aspace, uaddr2, observed, desired)?;
        if actual == observed {
            break observed;
        }
    };

    let mut woken = ipc::futex::wake(key1, wake_count);
    if wake_op_comparison(old, cmp, cmp_arg) {
        woken = woken.saturating_add(ipc::futex::wake(key2, wake_count2));
    }
    Ok(woken)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requeue_counts_must_fit_signed_int() {
        assert_eq!(validate_requeue_count(0), Ok(0));
        assert_eq!(validate_requeue_count(i32::MAX as u32),
                   Ok(i32::MAX as u32));
        assert_eq!(validate_requeue_count(i32::MAX as u32 + 1),
                   Err(ErrNo::EINVAL));
        assert_eq!(validate_requeue_count(u32::MAX),
                   Err(ErrNo::EINVAL));
    }

    #[test]
    fn wake_op_decode_and_apply_match_linux_encoding() {
        // FUTEX_OP(FUTEX_OP_ADD, 2, FUTEX_OP_CMP_EQ, 7)
        let encoded = (FUTEX_OP_ADD << 28) | (FUTEX_OP_CMP_EQ << 24) | (2 << 12) | 7;
        let (op, cmp, op_arg, cmp_arg) = decode_wake_op(encoded).unwrap();
        assert_eq!(apply_wake_op(5, op, op_arg), 7);
        assert!(wake_op_comparison(7, cmp, cmp_arg));

        // 12-bit -1 is sign extended for both operation and comparison args.
        let encoded_negative =
            (FUTEX_OP_SET << 28) | (FUTEX_OP_CMP_EQ << 24) | (0xFFF << 12) | 0xFFF;
        let (op, cmp, op_arg, cmp_arg) = decode_wake_op(encoded_negative).unwrap();
        assert_eq!(apply_wake_op(0, op, op_arg), u32::MAX);
        assert!(wake_op_comparison(u32::MAX, cmp, cmp_arg));
    }
}

pub(crate) fn wake_user_addr(user_aspace : usize, uaddr : usize) -> usize {
    super::super::misc::bringup_stats::record_futex_wake_user_addr();
    // clear_child_tid 的 wake 需要同时尝试 private 和 shared 两种 key，
    // 因为等待者可能用任一种 flag（glibc 可能用 FUTEX_WAIT_BITSET 不带 PRIVATE 标志）
    let private_key = FutexKey::private(uaddr, user_aspace);
    let n1 = ipc::futex::wake_all(private_key);
    let n2 = nonprivate_futex_key_for_aspace(user_aspace, uaddr).map(|key| {
                                                                    if key == private_key {
                                                                        0
                                                                    } else {
                                                                        ipc::futex::wake_all(key)
                                                                    }
                                                                })
                                                                .unwrap_or(0);
    let total = n1 + n2;
    log::trace!("[pthread-debug] wake_user_addr uaddr={:#x} private={n1} shared={n2} \
                 total={total}",
                uaddr,);
    if total == 0 {
        super::super::misc::bringup_stats::record_futex_wake_zero_waiters();
        log::trace!("[pthread-debug] wake_user_addr uaddr={:#x} no waiters",
                    uaddr);
    }
    total
}

pub(crate) fn sys_futex(args : SyscallArgs) -> UserRet {
    let futex_op = args.arg(1) as u32;
    let uaddr = args.arg(0);
    let val = args.arg(2) as u32;
    let timeout_ptr = args.arg(3);
    let uaddr2 = args.arg(4);
    let val3 = args.arg(5) as u32;

    let cmd = futex_op & FUTEX_CMD_MASK;

    let result =
        if futex_op & FUTEX_CLOCK_REALTIME != 0 && cmd != FUTEX_WAIT && cmd != FUTEX_WAIT_BITSET {
            Err(ErrNo::ENOSYS)
        } else {
            match cmd {
                FUTEX_WAIT => futex_wait(uaddr,
                                         val,
                                         0,
                                         cmd,
                                         futex_op,
                                         timeout_ptr),
                FUTEX_WAIT_BITSET => match validate_futex_bitset(cmd, val3) {
                    Ok(()) => futex_wait(uaddr,
                                         val,
                                         val3,
                                         cmd,
                                         futex_op,
                                         timeout_ptr),
                    Err(e) => Err(e),
                },
                FUTEX_WAKE => futex_wake(uaddr,
                                         val,
                                         FUTEX_BITSET_MATCH_ALL,
                                         futex_op),
                FUTEX_WAKE_BITSET => match validate_futex_bitset(cmd, val3) {
                    Ok(()) => futex_wake(uaddr, val, val3, futex_op),
                    Err(e) => Err(e),
                },
                FUTEX_REQUEUE => futex_requeue(uaddr,
                                               val,
                                               timeout_ptr as u32,
                                               uaddr2,
                                               futex_op,
                                               None),
                FUTEX_CMP_REQUEUE => futex_requeue(uaddr,
                                                   val,
                                                   timeout_ptr as u32,
                                                   uaddr2,
                                                   futex_op,
                                                   Some(val3)),
                FUTEX_WAKE_OP => futex_wake_op(uaddr,
                                               val,
                                               timeout_ptr as u32,
                                               uaddr2,
                                               val3,
                                               futex_op),
                _ => Err(ErrNo::ENOSYS),
            }
        };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}
