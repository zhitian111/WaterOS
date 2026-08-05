//! 可选的低扰动 bring-up 计数器。

#[cfg(feature = "bringup-stats")]
mod enabled {
    use core::sync::atomic::{AtomicU64, Ordering};

    use wateros_base_config::task::MAX_CPUS;

    /// 避免不同 CPU 更新同一缓存线；128 字节同时覆盖常见的 64/128 字节缓存线。
    #[repr(align(128))]
    struct CpuCounters {
        clone_thread : AtomicU64,
        sys_exit : AtomicU64,
        reap_member_calls : AtomicU64,
        reap_member_tasks : AtomicU64,
        futex_wake_user_addr : AtomicU64,
        futex_wake_zero_waiters : AtomicU64,
        futex_wait_sleep : AtomicU64,
        futex_wait_eagain : AtomicU64,
        user_page_fault_handled : AtomicU64,
    }

    impl CpuCounters {
        const fn new() -> Self {
            Self { clone_thread : AtomicU64::new(0),
                   sys_exit : AtomicU64::new(0),
                   reap_member_calls : AtomicU64::new(0),
                   reap_member_tasks : AtomicU64::new(0),
                   futex_wake_user_addr : AtomicU64::new(0),
                   futex_wake_zero_waiters : AtomicU64::new(0),
                   futex_wait_sleep : AtomicU64::new(0),
                   futex_wait_eagain : AtomicU64::new(0),
                   user_page_fault_handled : AtomicU64::new(0) }
        }
    }

    const _ : () = assert!(core::mem::align_of::<CpuCounters>() >= 128);

    static CPU_COUNTERS : [CpuCounters; MAX_CPUS] = [const { CpuCounters::new() }; MAX_CPUS];

    #[derive(Default)]
    struct Snapshot {
        clone_thread : u64,
        sys_exit : u64,
        reap_member_calls : u64,
        reap_member_tasks : u64,
        futex_wake_user_addr : u64,
        futex_wake_zero_waiters : u64,
        futex_wait_sleep : u64,
        futex_wait_eagain : u64,
        user_page_fault_handled : u64,
    }

    #[inline]
    fn current() -> &'static CpuCounters {
        &CPU_COUNTERS[platform::arch::cpu::current_cpu_id().index()]
    }

    fn snapshot() -> Snapshot {
        let mut result = Snapshot::default();
        for counters in &CPU_COUNTERS {
            result.clone_thread += counters.clone_thread
                                           .load(Ordering::Relaxed);
            result.sys_exit += counters.sys_exit
                                       .load(Ordering::Relaxed);
            result.reap_member_calls += counters.reap_member_calls
                                                .load(Ordering::Relaxed);
            result.reap_member_tasks += counters.reap_member_tasks
                                                .load(Ordering::Relaxed);
            result.futex_wake_user_addr += counters.futex_wake_user_addr
                                                   .load(Ordering::Relaxed);
            result.futex_wake_zero_waiters += counters.futex_wake_zero_waiters
                                                      .load(Ordering::Relaxed);
            result.futex_wait_sleep += counters.futex_wait_sleep
                                               .load(Ordering::Relaxed);
            result.futex_wait_eagain += counters.futex_wait_eagain
                                                .load(Ordering::Relaxed);
            result.user_page_fault_handled += counters.user_page_fault_handled
                                                      .load(Ordering::Relaxed);
        }
        result
    }

    #[inline]
    pub(super) fn record_clone_thread() {
        current().clone_thread
                 .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_sys_exit() {
        current().sys_exit
                 .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_reap_member_threads(reaped : usize) {
        current().reap_member_calls
                 .fetch_add(1, Ordering::Relaxed);
        if reaped > 0 {
            current().reap_member_tasks
                     .fetch_add(reaped as u64, Ordering::Relaxed);
        }
    }

    #[inline]
    pub(super) fn record_futex_wake_user_addr() {
        current().futex_wake_user_addr
                 .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_futex_wake_zero_waiters() {
        current().futex_wake_zero_waiters
                 .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_futex_wait_sleep() {
        current().futex_wait_sleep
                 .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_futex_wait_eagain() {
        current().futex_wait_eagain
                 .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub(super) fn record_user_page_fault_handled() {
        current().user_page_fault_handled
                 .fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn log_summary() {
        let counters = snapshot();
        log::info!("[bringup-stats] clone_thread={} exit={} reap_calls={} reap_tasks={} \
                    futex_wake={} futex_wake_zero={} futex_sleep={} futex_eagain={} user_pf={}",
                   counters.clone_thread,
                   counters.sys_exit,
                   counters.reap_member_calls,
                   counters.reap_member_tasks,
                   counters.futex_wake_user_addr,
                   counters.futex_wake_zero_waiters,
                   counters.futex_wait_sleep,
                   counters.futex_wait_eagain,
                   counters.user_page_fault_handled);
    }
}

#[inline]
pub(crate) fn record_clone_thread() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_clone_thread();
}

#[inline]
pub(crate) fn record_sys_exit() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_sys_exit();
}

#[inline]
pub(crate) fn record_reap_member_threads(_reaped : usize) {
    #[cfg(feature = "bringup-stats")]
    enabled::record_reap_member_threads(_reaped);
}

#[inline]
pub(crate) fn record_futex_wake_user_addr() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_futex_wake_user_addr();
}

#[inline]
pub(crate) fn record_futex_wake_zero_waiters() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_futex_wake_zero_waiters();
}

#[inline]
pub(crate) fn record_futex_wait_sleep() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_futex_wait_sleep();
}

#[inline]
pub(crate) fn record_futex_wait_eagain() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_futex_wait_eagain();
}

/// 记录一次用户态惰性缺页成功处理（线程栈 mmap 等）。
#[inline]
pub fn record_user_page_fault_handled() {
    #[cfg(feature = "bringup-stats")]
    enabled::record_user_page_fault_handled();
}

/// 输出当前累计计数（脚本切换等检查点可调用）。
#[inline]
pub fn log_thread_bringup_stats_summary() {
    #[cfg(feature = "bringup-stats")]
    enabled::log_summary();
}
