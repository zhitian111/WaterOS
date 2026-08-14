//! 全局信号服务。
//!
//! `ARCH:` 此 facade 是 signal API 的状态边界：它隐藏唯一的 `SignalRegistry` 锁，并把
//! 必须原子完成的 registry 操作合并为领域操作。它不直接调度、唤醒或终止任务；这些副作用
//! 由调用者根据返回的 [`SignalDispatch`] 在锁释放后执行。

use alloc::vec::Vec;

use api_v0::{
    AlternateSignalStack, IntervalTimerSpec, PosixTimerClock, SignalAction, SignalDispatch,
    SignalEffect, SignalResult, SignalSet, TakenPendingSignal,
};
use spin::Mutex;

use crate::registry::SignalRegistry;

/// `LOCK:` 唯一的信号状态实例，串行化所有进程/线程信号表操作。
///
/// `SMP:` 该锁只保护信号状态，不覆盖 scheduler、process registry、用户地址空间或 IPI。
/// 因此不得在持锁闭包中进行阻塞、调度或可能回调本模块的操作。
static REGISTRY : Mutex<SignalRegistry> = Mutex::new(SignalRegistry::new());

/// `LOCK:` 以最小临界区访问注册表。
///
/// 闭包只应完成纯信号状态读写；`spin::Mutex` 不可重入，不能从闭包中再次调用本 facade。
fn with_registry<R>(f : impl FnOnce(&mut SignalRegistry) -> R) -> R {
    let mut registry = REGISTRY.lock();
    f(&mut registry)
}

/// `FLOW:` 确保进程及其已存在的线程均已登记；可安全重复调用。
///
/// 进程创建或从 task/process 表恢复信号状态时使用。`leader_task_id` 必须属于 `pid`，其余
/// `(task_id, tid)` 作为同进程线程补登记。
pub fn ensure_process<I>(pid : usize,
                         leader_task_id : usize,
                         leader_tid : usize,
                         threads : I)
                         -> SignalResult<()>
    where I : IntoIterator<Item = (usize, usize)>
{
    with_registry(|registry| {
        registry.register_process(pid, leader_task_id, leader_tid);
        for (task_id, tid) in threads {
            if task_id != leader_task_id && !registry.has_thread(task_id) {
                registry.register_thread(leader_task_id, task_id, tid)?;
            }
        }
        Ok(())
    })
}

/// `FLOW:` 建立 `fork` 子进程的独立信号状态（复制 disposition、父线程 mask 与备用栈）。
pub fn fork_process(parent_task_id : usize,
                    child_pid : usize,
                    child_task_id : usize,
                    child_tid : usize)
                    -> SignalResult<()> {
    with_registry(|registry| {
        registry.fork_process(parent_task_id,
                              child_pid,
                              child_task_id,
                              child_tid)
    })
}

/// `FLOW:` `CLONE_THREAD` 路径登记同进程线程；只继承调用线程的阻塞 mask。
pub fn register_thread(parent_task_id : usize, task_id : usize, tid : usize) -> SignalResult<()> {
    with_registry(|registry| registry.register_thread(parent_task_id, task_id, tid))
}

/// 删除 exec 淘汰的线程状态，并重置存活线程所属进程的 exec-sensitive 状态。
pub fn exec_process<I>(task_id : usize, removed_task_ids : I) -> SignalResult<()>
    where I : IntoIterator<Item = usize> {
    with_registry(|registry| {
        for removed_task_id in removed_task_ids {
            registry.drop_thread(removed_task_id);
        }
        registry.exec_process(task_id)
    })
}

pub fn drop_thread(task_id : usize) {
    with_registry(|registry| registry.drop_thread(task_id));
}

pub fn drop_process(pid : usize) {
    with_registry(|registry| registry.drop_process(pid));
}

pub fn drop_thread_and_empty_process(task_id : usize) {
    with_registry(|registry| registry.drop_thread_and_empty_process(task_id));
}

/// `FLOW:` 退出路径删除线程；`last_thread` 时一并清除共享进程信号状态与 timers。
pub fn exit_thread(task_id : usize, pid : usize, last_thread : bool) {
    with_registry(|registry| {
        registry.drop_thread(task_id);
        if last_thread {
            registry.drop_process(pid);
        }
    });
}

/// `FLOW:` 向一个指定线程投递信号，并返回调用方应在锁外执行的唤醒/停止/继续意图。
pub fn send_thread(task_id : usize, signal : usize) -> SignalResult<SignalDispatch> {
    with_registry(|registry| registry.send_thread(task_id, signal))
}

/// `FLOW:` 向进程投递信号；registry 选择候选线程，但不直接操作 task/scheduler。
pub fn send_process(pid : usize, signal : usize) -> SignalResult<SignalDispatch> {
    with_registry(|registry| registry.send_process(pid, signal))
}

pub fn pending(task_id : usize) -> SignalResult<SignalSet> {
    with_registry(|registry| registry.pending(task_id))
}

pub fn has_deliverable(task_id : usize) -> SignalResult<bool> {
    with_registry(|registry| registry.has_deliverable(task_id))
}

pub fn take_pending(task_id : usize, wait_set : SignalSet) -> Option<usize> {
    with_registry(|registry| registry.take_pending(task_id, wait_set))
}

pub fn take_pending_record(task_id : usize,
                           wait_set : SignalSet)
                           -> Option<TakenPendingSignal> {
    with_registry(|registry| registry.take_pending_record(task_id, wait_set))
}

pub fn restore_pending_record(task_id : usize,
                              record : TakenPendingSignal)
                              -> SignalResult<()> {
    with_registry(|registry| registry.restore_pending_record(task_id, record))
}

/// `FLOW:` 目标线程在返回用户态前消费一个可交付信号并得到最终效果。
///
/// `ABI:` `Handler` 的 signal frame 由 syscall/trap 层构造；`Terminate`、`Stop`、`Continue`
/// 也必须由调用者在锁外落实到任务状态。
pub fn take_deliverable(task_id : usize) -> Option<SignalEffect> {
    with_registry(|registry| registry.take_deliverable(task_id))
}

pub fn take_sigkill(task_id : usize) -> bool {
    with_registry(|registry| registry.take_sigkill(task_id))
}

pub fn get_action(task_id : usize, signal : usize) -> SignalResult<SignalAction> {
    with_registry(|registry| registry.get_action(task_id, signal))
}

pub fn set_action(task_id : usize,
                  signal : usize,
                  action : SignalAction)
                  -> SignalResult<SignalAction> {
    with_registry(|registry| registry.set_action(task_id, signal, action))
}

pub fn update_mask(task_id : usize,
                   how : usize,
                   set : Option<SignalSet>)
                   -> SignalResult<SignalSet> {
    with_registry(|registry| registry.update_mask(task_id, how, set))
}

pub fn begin_sigsuspend(task_id : usize, mask : SignalSet) -> SignalResult<()> {
    with_registry(|registry| registry.begin_sigsuspend(task_id, mask))
}

pub fn end_sigsuspend(task_id : usize) -> SignalResult<()> {
    with_registry(|registry| registry.end_sigsuspend(task_id))
}

pub fn begin_poll_sigmask(task_id : usize, mask : SignalSet) -> SignalResult<()> {
    with_registry(|registry| registry.begin_poll_sigmask(task_id, mask))
}

pub fn end_poll_sigmask(task_id : usize) -> SignalResult<()> {
    with_registry(|registry| registry.end_poll_sigmask(task_id))
}

pub fn begin_signal_wait(task_id : usize, wait_set : SignalSet) -> SignalResult<()> {
    with_registry(|registry| registry.begin_signal_wait(task_id, wait_set))
}

pub fn end_signal_wait(task_id : usize) -> SignalResult<()> {
    with_registry(|registry| registry.end_signal_wait(task_id))
}

pub fn pending_in(task_id : usize, set : SignalSet) -> SignalResult<bool> {
    with_registry(|registry| {
        registry.pending(task_id)
                .map(|pending| {
                    !pending.intersection(set)
                            .is_empty()
                })
    })
}

pub fn alternate_stack(task_id : usize) -> SignalResult<AlternateSignalStack> {
    with_registry(|registry| registry.alternate_stack(task_id))
}

pub fn replace_alternate_stack(task_id : usize,
                               replacement : AlternateSignalStack)
                               -> SignalResult<AlternateSignalStack> {
    with_registry(|registry| registry.replace_alternate_stack(task_id, replacement))
}

pub fn enter_signal_frame(task_id : usize, on_alternate_stack : bool) -> SignalResult<()> {
    with_registry(|registry| registry.enter_signal_frame(task_id, on_alternate_stack))
}

/// `ABI:` `rt_sigreturn` 路径恢复信号 mask，并离开可能使用的备用信号栈。
///
/// `frame_sp` 是恢复前 signal frame 的栈指针，用来判断这次返回是否消耗了备用栈帧。
pub fn leave_signal_frame(task_id : usize,
                          restored_mask : SignalSet,
                          frame_sp : usize)
                          -> SignalResult<()> {
    with_registry(|registry| {
        let alternate_stack = registry.alternate_stack(task_id)?;
        registry.replace_mask(task_id, restored_mask)?;
        registry.leave_signal_frame(task_id,
                                    alternate_stack.contains(frame_sp))
    })
}

/// `FLOW:` 配置 interval timer；返回替换前的剩余时间。
pub fn set_timer(pid : usize,
                 which : usize,
                 spec : IntervalTimerSpec,
                 monotonic_ns : u128)
                 -> SignalResult<IntervalTimerSpec> {
    with_registry(|registry| registry.set_timer(pid, which, spec, monotonic_ns))
}

pub fn get_timer(pid : usize,
                 which : usize,
                 monotonic_ns : u128)
                 -> SignalResult<IntervalTimerSpec> {
    with_registry(|registry| registry.get_timer(pid, which, monotonic_ns))
}

pub fn create_posix_timer(pid : usize,
                          clock : PosixTimerClock,
                          signal : usize)
                          -> SignalResult<usize> {
    with_registry(|registry| registry.create_posix_timer(pid, clock, signal))
}

pub fn set_posix_timer(pid : usize,
                       timer_id : usize,
                       spec : IntervalTimerSpec,
                       monotonic_ns : u128,
                       realtime_ns : u128,
                       absolute : bool)
                       -> SignalResult<IntervalTimerSpec> {
    with_registry(|registry| {
        registry.set_posix_timer(pid,
                                 timer_id,
                                 spec,
                                 monotonic_ns,
                                 realtime_ns,
                                 absolute)
    })
}

pub fn get_posix_timer(pid : usize,
                       timer_id : usize,
                       monotonic_ns : u128,
                       realtime_ns : u128)
                       -> SignalResult<IntervalTimerSpec> {
    with_registry(|registry| registry.get_posix_timer(pid, timer_id, monotonic_ns, realtime_ns))
}

pub fn get_posix_timer_overrun(pid : usize, timer_id : usize) -> SignalResult<i32> {
    with_registry(|registry| registry.get_posix_timer_overrun(pid, timer_id))
}

pub fn delete_posix_timer(pid : usize, timer_id : usize) -> SignalResult<()> {
    with_registry(|registry| registry.delete_posix_timer(pid, timer_id))
}

/// `SMP:` 对实际运行的进程累计 CPU 时间，并返回到期 timer 的 dispatch；调用者在锁外处理它们。
pub fn account_cpu(pid : usize,
                   user_delta_ns : u128,
                   total_delta_ns : u128)
                   -> SignalResult<Vec<(SignalDispatch, usize)>> {
    with_registry(|registry| registry.account_cpu(pid, user_delta_ns, total_delta_ns))
}

/// `FLOW:` 扫描到期的 `ITIMER_REAL`，生成对应 `SIGALRM` dispatch。
pub fn expire_realtime(monotonic_ns : u128) -> Vec<SignalDispatch> {
    with_registry(|registry| registry.expire_realtime(monotonic_ns))
}

pub fn expire_posix_timers(monotonic_ns : u128,
                           realtime_ns : u128)
                           -> Vec<(SignalDispatch, usize)> {
    with_registry(|registry| registry.expire_posix_timers(monotonic_ns, realtime_ns))
}
