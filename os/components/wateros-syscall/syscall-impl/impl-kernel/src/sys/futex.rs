//! `futex(2)` — 用户态快速互斥锁原语（单核实现）。
//!
//! 在单核系统上无需处理多 CPU 并发，原子性由 syscall 串行执行自然保证。
//! 等待/唤醒通过 [`wateros_task::WaitQueue`] 实现。

use alloc::collections::BTreeMap;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::WaitQueue;

use crate::user_copy::copy_from_user;

// ── futex 操作码 ──────────────────────────────────────────────────

const FUTEX_WAIT : u32 = 0;
const FUTEX_WAKE : u32 = 1;
const FUTEX_WAIT_BITSET : u32 = 9;
const FUTEX_WAKE_BITSET : u32 = 10;
const FUTEX_REQUEUE : u32 = 3;

const FUTEX_PRIVATE_FLAG : u32 = 128;
const FUTEX_CMD_MASK : u32 = !(FUTEX_PRIVATE_FLAG);

/// 按 futex 用户地址派生的队列键；同页不同 futex 变量必须互不影响。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FutexKey(usize);

impl FutexKey {
    fn from_uaddr(uaddr : usize) -> Self { Self(uaddr) }
}

// ── 全局 futex 表（单核下无需 Mutex） ─────────────────────────────

/// 全局 futex 表（单核下无需 Mutex）。
///
/// SAFETY: 单核系统，所有 futex 操作在 syscall 上下文中串行执行。
struct FutexTable {
    map : BTreeMap<usize, WaitQueue>,
}

unsafe impl Sync for FutexTable {}

static TABLE : FutexTable = FutexTable { map : BTreeMap::new() };

fn with_table<R>(f : impl FnOnce(&mut FutexTable) -> R) -> R {
    // SAFETY: 单核串行访问，无数据竞争。
    unsafe { f(&mut *(&raw const TABLE as *const FutexTable as *mut FutexTable)) }
}

/// 按 key 获取或创建 WaitQueue。
fn get_queue(key : FutexKey) -> WaitQueue {
    with_table(|table| {
        *table.map
              .entry(key.0)
              .or_insert_with(WaitQueue::new)
    })
}

// ── 用户内存读取 ─────────────────────────────────────────────────

fn read_user_u32(uaddr : usize) -> Result<u32, ErrNo> {
    let mut val : u32 = 0;
    let buf = unsafe { core::slice::from_raw_parts_mut((&raw mut val) as *mut u8, 4) };
    if copy_from_user(buf, uaddr)? != 4 {
        return Err(ErrNo::EFAULT);
    }
    Ok(val)
}

// ── 核心操作 ─────────────────────────────────────────────────────

fn futex_wait(uaddr : usize, val : u32, bitset : u32) -> Result<usize, ErrNo> {
    // bitset 为 0 或全匹配则视为无条件（Linux 行为）
    let _ = bitset;

    let cur = read_user_u32(uaddr)?;
    if cur != val {
        return Err(ErrNo::EAGAIN);
    }

    let key = FutexKey::from_uaddr(uaddr);
    let wq = get_queue(key);
    // wait_current_while 在调度临界区内复查条件并提供原子性，
    // 避免 "检查→睡眠" 窗口内丢失唤醒。
    wq.wait_current_while(|| read_user_u32(uaddr).map_or(false, |v| v == val));
    Ok(0)
}

fn futex_wake(uaddr : usize, max_wake : u32, bitset : u32) -> Result<usize, ErrNo> {
    let _ = bitset;
    let key = FutexKey::from_uaddr(uaddr);
    let wq = get_queue(key);

    let limit = if max_wake == 0 { 1 } else { max_wake as usize };
    let mut woken = 0;
    for _ in 0..limit {
        if wq.wake_one()
             .is_none()
        {
            break;
        }
        woken += 1;
    }
    Ok(woken)
}

pub(crate) fn wake_user_addr(uaddr : usize) -> usize {
    let key = FutexKey::from_uaddr(uaddr);
    let wq = get_queue(key);
    wq.wake_all()
}

// ── 公开入口 ─────────────────────────────────────────────────────

pub(crate) fn sys_futex(args : SyscallArgs) -> UserRet {
    let futex_op = args.arg(1) as u32;
    let uaddr = args.arg(0);
    let val = args.arg(2) as u32;
    let timeout_or_val2 = args.arg(3) as u32;
    let val3 = args.arg(5) as u32;

    let cmd = futex_op & FUTEX_CMD_MASK;

    let result = match cmd {
        FUTEX_WAIT => futex_wait(uaddr, val, 0),
        FUTEX_WAIT_BITSET => futex_wait(uaddr, val, timeout_or_val2),
        FUTEX_WAKE => futex_wake(uaddr, val, 0),
        FUTEX_WAKE_BITSET => futex_wake(uaddr, val, val3),
        FUTEX_REQUEUE => {
            // FUTEX_CMP_REQUEUE 等暂不支持，返回 ENOSYS
            Err(ErrNo::ENOSYS)
        }
        _ => Err(ErrNo::ENOSYS),
    };

    match result {
        Ok(n) => UserRet::from_success(n),
        Err(e) => UserRet::from_error(e),
    }
}
