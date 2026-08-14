//! 调度策略与 CPU 亲和性原语。

use api_v0::{
    CpuMask, Priority, ProcessId, SchedError, SchedPolicy, TaskId, ThreadId, NICE_MAX, NICE_MIN,
    SCHED_CPU_MASK_MIN_BYTES, SCHED_CPU_MASK_RET_BYTES,
};

use crate::scheduler::{self};

fn existing_task_id(task_id : TaskId) -> Option<TaskId> {
    scheduler::task_snapshot(task_id).map(|_| task_id)
}

/// 将 Linux `pid`（0 = 当前线程；正数 = 用户可见 tid/pid）解析为内部 [`TaskId`]。
pub fn resolve_sched_pid(pid : isize) -> Result<TaskId, SchedError> {
    if pid == 0 {
        return scheduler::current_task_id().ok_or(SchedError::NoSuchTask);
    }
    if pid < 0 {
        return Err(SchedError::InvalidArg);
    }

    let raw = pid as usize;
    if let Some(task_id) =
        crate::task_id_for_thread(ThreadId::from_raw(raw)).and_then(existing_task_id)
    {
        return Ok(task_id);
    }
    if let Some(task_id) =
        crate::leader_task_for_process(ProcessId::from_raw(raw)).and_then(existing_task_id)
    {
        return Ok(task_id);
    }

    Err(SchedError::NoSuchTask)
}

/// 查询任务的有效调度策略。
pub fn get_scheduler_policy(task_id : TaskId) -> Result<SchedPolicy, SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::policy(task_id)
}

/// 查询任务的调度参数（优先级）。
pub fn get_param(task_id : TaskId) -> Result<Priority, SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::priority(task_id)
}

/// 返回写入 userspace 的有效 mask 字节数。
#[must_use]
pub const fn cpu_affinity_ret_bytes() -> usize { SCHED_CPU_MASK_RET_BYTES }

/// 校验 affinity 查询缓冲区长度。
pub fn validate_cpu_affinity_buf_len(cpusetsize : usize) -> Result<(), SchedError> {
    if cpusetsize < SCHED_CPU_MASK_MIN_BYTES {
        Err(SchedError::InvalidArg)
    } else {
        Ok(())
    }
}

/// 设置调度策略。
pub fn set_scheduler_policy(task_id : TaskId,
                            policy : SchedPolicy,
                            priority : Priority)
                            -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    validate_policy_param(policy, priority)?;
    scheduler::apply_sched_policy_change(task_id, policy, priority)
}

/// 设置调度参数（保持当前 policy 不变）。
pub fn set_param(task_id : TaskId, priority : Priority) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    let policy = get_scheduler_policy(task_id)?;
    validate_policy_param(policy, priority)?;
    scheduler::apply_sched_policy_change(task_id, policy, priority)
}

/// 设置任务 CPU 亲和性。目标 mask 必须只包含已配置且至少一个 online CPU。
pub fn set_affinity(task_id : TaskId, mask : CpuMask) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::set_affinity(task_id, mask)
}
pub fn get_affinity(task_id : TaskId) -> Result<CpuMask, SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::get_affinity(task_id)
}

/// 设置线程级 nice；新权重会在运行任务的下一 tick 生效。
pub fn set_nice(task_id : TaskId, nice : i8) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    if !(NICE_MIN..=NICE_MAX).contains(&nice) {
        return Err(SchedError::InvalidArg);
    }
    scheduler::set_nice(task_id, nice)
}

/// 查询线程级 nice。
pub fn get_nice(task_id : TaskId) -> Result<i8, SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::get_nice(task_id)
}

/// 设置线程级 Linux I/O 优先级编码；fork/clone 从 TCB 自动继承。
pub fn set_io_priority(task_id : TaskId, io_priority : u16) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::set_io_priority(task_id, io_priority)
}

/// 查询线程级 Linux I/O 优先级编码。
pub fn get_io_priority(task_id : TaskId) -> Result<u16, SchedError> {
    ensure_task_exists(task_id)?;
    scheduler::get_io_priority(task_id)
}

// 确认 task 仍存在于调度器 registry。
fn ensure_task_exists(task_id : TaskId) -> Result<(), SchedError> {
    if scheduler::task_snapshot(task_id).is_some() {
        Ok(())
    } else {
        Err(SchedError::NoSuchTask)
    }
}

// 按策略校验 priority 取值范围。
fn validate_policy_param(policy : SchedPolicy, priority : Priority) -> Result<(), SchedError> {
    scheduler::validate_policy_param(policy, priority)
}
