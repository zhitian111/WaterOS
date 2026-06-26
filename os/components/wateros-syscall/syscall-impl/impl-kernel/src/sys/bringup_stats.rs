//! pthread / 线程 bring-up 临时计数器，用于定位 libcbench serial2 等压测热点。

use core::sync::atomic::{AtomicU64, Ordering};

static CLONE_THREAD : AtomicU64 = AtomicU64::new(0);
static SYS_EXIT : AtomicU64 = AtomicU64::new(0);
static REAP_MEMBER_CALLS : AtomicU64 = AtomicU64::new(0);
static REAP_MEMBER_TASKS : AtomicU64 = AtomicU64::new(0);
static FUTEX_WAKE_USER_ADDR : AtomicU64 = AtomicU64::new(0);
static FUTEX_WAKE_ZERO_WAITERS : AtomicU64 = AtomicU64::new(0);
static FUTEX_WAIT_SLEEP : AtomicU64 = AtomicU64::new(0);
static FUTEX_WAIT_EAGAIN : AtomicU64 = AtomicU64::new(0);
static USER_PAGE_FAULT_HANDLED : AtomicU64 = AtomicU64::new(0);

const LOG_EVERY : u64 = 512;

fn maybe_log_summary(total : u64) {
    if total > 0 && total % LOG_EVERY == 0 {
        log::trace!("[bringup-stats] clone_thread={} exit={} reap_calls={} reap_tasks={} \
                     futex_wake={} futex_wake_zero={} futex_sleep={} futex_eagain={} user_pf={}",
                    CLONE_THREAD.load(Ordering::Relaxed),
                    SYS_EXIT.load(Ordering::Relaxed),
                    REAP_MEMBER_CALLS.load(Ordering::Relaxed),
                    REAP_MEMBER_TASKS.load(Ordering::Relaxed),
                    FUTEX_WAKE_USER_ADDR.load(Ordering::Relaxed),
                    FUTEX_WAKE_ZERO_WAITERS.load(Ordering::Relaxed),
                    FUTEX_WAIT_SLEEP.load(Ordering::Relaxed),
                    FUTEX_WAIT_EAGAIN.load(Ordering::Relaxed),
                    USER_PAGE_FAULT_HANDLED.load(Ordering::Relaxed));
    }
}

pub(crate) fn record_clone_thread() {
    let total = CLONE_THREAD.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

pub(crate) fn record_sys_exit() {
    let total = SYS_EXIT.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

pub(crate) fn record_reap_member_threads(reaped : usize) {
    let calls = REAP_MEMBER_CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    if reaped > 0 {
        let _ = REAP_MEMBER_TASKS.fetch_add(reaped as u64, Ordering::Relaxed);
    }
    maybe_log_summary(calls);
}

pub(crate) fn record_futex_wake_user_addr() {
    let total = FUTEX_WAKE_USER_ADDR.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

pub(crate) fn record_futex_wake_zero_waiters() {
    let total = FUTEX_WAKE_ZERO_WAITERS.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

pub(crate) fn record_futex_wait_sleep() {
    let total = FUTEX_WAIT_SLEEP.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

pub(crate) fn record_futex_wait_eagain() {
    let total = FUTEX_WAIT_EAGAIN.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

/// 记录一次用户态惰性缺页成功处理（线程栈 mmap 等）。
pub fn record_user_page_fault_handled() {
    let total = USER_PAGE_FAULT_HANDLED.fetch_add(1, Ordering::Relaxed) + 1;
    maybe_log_summary(total);
}

/// 输出当前累计计数（脚本切换等检查点可调用）。
pub fn log_thread_bringup_stats_summary() {
    log::info!("[bringup-stats] clone_thread={} exit={} reap_calls={} reap_tasks={} \
                futex_wake={} futex_wake_zero={} futex_sleep={} futex_eagain={} user_pf={}",
               CLONE_THREAD.load(Ordering::Relaxed),
               SYS_EXIT.load(Ordering::Relaxed),
               REAP_MEMBER_CALLS.load(Ordering::Relaxed),
               REAP_MEMBER_TASKS.load(Ordering::Relaxed),
               FUTEX_WAKE_USER_ADDR.load(Ordering::Relaxed),
               FUTEX_WAKE_ZERO_WAITERS.load(Ordering::Relaxed),
               FUTEX_WAIT_SLEEP.load(Ordering::Relaxed),
               FUTEX_WAIT_EAGAIN.load(Ordering::Relaxed),
               USER_PAGE_FAULT_HANDLED.load(Ordering::Relaxed));
}
