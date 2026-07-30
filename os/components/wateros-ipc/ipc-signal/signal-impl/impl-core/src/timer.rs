//! Interval timer、POSIX timer 与 CPU timer 逻辑。
//!
//! `FLOW:` 到期处理只修改信号状态并返回 `SignalDispatch`；调用者必须在 registry 锁释放后
//! 执行唤醒、停止、终止或跨核通知。

use alloc::vec::Vec;

use api_v0::{
    valid_signal, IntervalTimerSpec, PosixTimerClock, SignalDispatch, SignalError, SignalResult,
    ITIMER_REAL, SIGALRM, SIGPROF, SIGVTALRM,
};

use crate::registry::SignalRegistry;
use crate::state::{PosixTimerState, RealDeadlineEntry};

impl SignalRegistry {
    /// `FLOW:` 设置进程 interval timer（`setitimer`），并为 `ITIMER_REAL` 登记 deadline 索引。
    ///
    /// `INVARIANT:` 旧 deadline 不立即删除；generation 使其在到期扫描时成为无效项。
    pub fn set_timer(&mut self,
                     pid : usize,
                     which : usize,
                     spec : IntervalTimerSpec,
                     monotonic_ns : u128)
                     -> SignalResult<IntervalTimerSpec> {
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        let now = process.timer_clock(which, monotonic_ns)?;
        let old = process.timer(which)?
                         .remaining(now);
        let timer = process.timer_mut(which)?;
        timer.replace(spec, now);
        if which == ITIMER_REAL {
            if let Some(deadline) = timer.deadline_ns {
                self.real_deadlines
                    .entry(deadline)
                    .or_default()
                    .push(RealDeadlineEntry { pid,
                                              generation : timer.generation });
            }
        }
        Ok(old)
    }

    /// 查询进程 interval timer 剩余时间（`getitimer`）。
    pub fn get_timer(&self,
                     pid : usize,
                     which : usize,
                     monotonic_ns : u128)
                     -> SignalResult<IntervalTimerSpec> {
        let process = self.processes
                          .get(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        let now = process.timer_clock(which, monotonic_ns)?;
        Ok(process.timer(which)?
                  .remaining(now))
    }

    /// `DATA:` 分配进程内 POSIX timer ID；ID 只在该 PID 的表中有效。
    pub fn create_posix_timer(&mut self,
                              pid : usize,
                              clock : PosixTimerClock,
                              signal : usize)
                              -> SignalResult<usize> {
        if !valid_signal(signal) {
            return Err(SignalError::InvalidSignal);
        }
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        for _ in 0..=process.posix_timers
                            .len()
        {
            let timer_id = process.next_posix_timer_id;
            process.next_posix_timer_id = process.next_posix_timer_id
                                                 .wrapping_add(1) &
                                          (i32::MAX as usize);
            if !process.posix_timers
                       .contains_key(&timer_id)
            {
                process.posix_timers
                       .insert(timer_id,
                               PosixTimerState::new(clock, signal));
                return Ok(timer_id);
            }
        }
        Err(SignalError::InvalidTimer)
    }

    pub fn set_posix_timer(&mut self,
                           pid : usize,
                           timer_id : usize,
                           spec : IntervalTimerSpec,
                           monotonic_ns : u128,
                           realtime_ns : u128,
                           absolute : bool)
                           -> SignalResult<IntervalTimerSpec> {
        let timer = self.processes
                        .get_mut(&pid)
                        .ok_or(SignalError::NoSuchProcess)?
                        .posix_timers
                        .get_mut(&timer_id)
                        .ok_or(SignalError::NoSuchTimer)?;
        let old = timer.remaining(monotonic_ns, realtime_ns);
        let now = timer.now(monotonic_ns, realtime_ns);
        timer.interval_ns = spec.interval_ns;
        timer.deadline_ns = if spec.value_ns == 0 {
            None
        } else if absolute {
            Some(spec.value_ns)
        } else {
            Some(now.saturating_add(spec.value_ns))
        };
        timer.overrun = 0;
        Ok(old)
    }

    pub fn get_posix_timer(&self,
                           pid : usize,
                           timer_id : usize,
                           monotonic_ns : u128,
                           realtime_ns : u128)
                           -> SignalResult<IntervalTimerSpec> {
        let timer = self.processes
                        .get(&pid)
                        .ok_or(SignalError::NoSuchProcess)?
                        .posix_timers
                        .get(&timer_id)
                        .ok_or(SignalError::NoSuchTimer)?;
        Ok(timer.remaining(monotonic_ns, realtime_ns))
    }

    pub fn get_posix_timer_overrun(&self, pid : usize, timer_id : usize) -> SignalResult<i32> {
        self.processes
            .get(&pid)
            .ok_or(SignalError::NoSuchProcess)?
            .posix_timers
            .get(&timer_id)
            .map(|timer| timer.overrun)
            .ok_or(SignalError::NoSuchTimer)
    }

    pub fn delete_posix_timer(&mut self, pid : usize, timer_id : usize) -> SignalResult<()> {
        self.processes
            .get_mut(&pid)
            .ok_or(SignalError::NoSuchProcess)?
            .posix_timers
            .remove(&timer_id)
            .map(|_| ())
            .ok_or(SignalError::NoSuchTimer)
    }

    /// `FLOW:` 扫描 POSIX timer，记录 overrun 并把到期项转成普通进程信号投递。
    pub fn expire_posix_timers(&mut self,
                               monotonic_ns : u128,
                               realtime_ns : u128)
                               -> Vec<(SignalDispatch, usize)> {
        let mut expired = Vec::new();
        for (pid, process) in self.processes
                                  .iter_mut()
        {
            for timer in process.posix_timers
                                .values_mut()
            {
                let Some(deadline) = timer.deadline_ns else {
                    continue;
                };
                let now = timer.now(monotonic_ns, realtime_ns);
                if deadline > now {
                    continue;
                }
                let expirations = if timer.interval_ns == 0 {
                    timer.deadline_ns = None;
                    1
                } else {
                    let expirations = now.saturating_sub(deadline) / timer.interval_ns + 1;
                    timer.deadline_ns =
                        Some(deadline.saturating_add(expirations.saturating_mul(timer.interval_ns)));
                    expirations
                };
                timer.overrun = i32::try_from(expirations.saturating_sub(1)).unwrap_or(i32::MAX);
                expired.push((*pid, timer.signal));
            }
        }
        expired.into_iter()
               .filter_map(|(pid, signal)| {
                   self.send_process(pid, signal)
                       .ok()
                       .map(|dispatch| (dispatch, signal))
               })
               .collect()
    }

    /// `SMP:` 累计 CPU 时间并触发 virtual/prof timer 到期。
    ///
    /// 调用者必须只为实际正在某 CPU 上运行的该 PID 记账；本函数不拥有全局 tick 语义。
    pub fn account_cpu(&mut self,
                       pid : usize,
                       user_delta_ns : u128,
                       total_delta_ns : u128)
                       -> SignalResult<Vec<(SignalDispatch, usize)>> {
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        process.user_cpu_ns = process.user_cpu_ns
                                     .saturating_add(user_delta_ns);
        process.total_cpu_ns = process.total_cpu_ns
                                      .saturating_add(total_delta_ns);
        let virtual_expired = process.virtual_timer
                                     .expire(process.user_cpu_ns);
        let prof_expired = process.prof
                                  .expire(process.total_cpu_ns);
        let mut dispatches = Vec::new();
        if virtual_expired {
            dispatches.push((self.send_process(pid, SIGVTALRM)?, SIGVTALRM));
        }
        if prof_expired {
            dispatches.push((self.send_process(pid, SIGPROF)?, SIGPROF));
        }
        Ok(dispatches)
    }

    /// `FLOW:` 扫描已到期 realtime timer 并投递 `SIGALRM`。
    ///
    /// 从 BTreeMap 删除 bucket 后以 `(pid, generation, deadline)` 三元条件验证，保证重设
    /// timer 留下的陈旧索引项不会错误触发。
    pub fn expire_realtime(&mut self, monotonic_ns : u128) -> Vec<SignalDispatch> {
        let deadlines : Vec<u128> = self.real_deadlines
                                        .range(..=monotonic_ns)
                                        .map(|(deadline, _)| *deadline)
                                        .collect();
        let mut dispatches = Vec::new();
        for deadline in deadlines {
            let entries = self.real_deadlines
                              .remove(&deadline)
                              .unwrap_or_default();
            for entry in entries {
                let Some(process) = self.processes
                                        .get_mut(&entry.pid)
                else {
                    continue;
                };
                if process.real
                          .generation !=
                   entry.generation ||
                   process.real
                          .deadline_ns !=
                   Some(deadline)
                {
                    continue;
                }
                if !process.real
                           .expire(monotonic_ns)
                {
                    continue;
                }
                if let Some(next_deadline) = process.real
                                                    .deadline_ns
                {
                    self.real_deadlines
                        .entry(next_deadline)
                        .or_default()
                        .push(RealDeadlineEntry { pid : entry.pid,
                                                  generation : process.real
                                                                      .generation });
                }
                if let Ok(dispatch) = self.send_process(entry.pid, SIGALRM) {
                    dispatches.push(dispatch);
                }
            }
        }
        dispatches
    }
}
