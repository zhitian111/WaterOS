//! 调度策略与 CPU 亲和性 **原语**。

use api_v0::{
    ProcessId, SchedError, SchedParam, SchedPolicy, ThreadId, SCHED_CPU_MASK_MIN_BYTES,
    SCHED_CPU_MASK_RET_BYTES, TaskId,
};

use crate::scheduler::{self, SchedPolicyChangeAction};

#[inline]
fn existing_task_id(task_id: TaskId) -> Option<TaskId> {
    scheduler::task_snapshot(task_id).map(|_| task_id)
}

/// 将 Linux `pid`（0 = 当前线程；正数 = 用户可见 tid/pid）解析为内部 [`TaskId`]。
#[inline]
pub fn resolve_sched_pid(pid: isize) -> Result<TaskId, SchedError> {
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
#[inline]
pub fn get_scheduler(task_id: TaskId) -> Result<SchedPolicy, SchedError> {
    ensure_task_exists(task_id)?;
    Ok(scheduler::task_snapshot(task_id)
        .expect("task exists")
        .sched_policy)
}

/// 查询任务的调度参数。
#[inline]
pub fn get_param(task_id: TaskId) -> Result<SchedParam, SchedError> {
    ensure_task_exists(task_id)?;
    Ok(SchedParam {
        priority: scheduler::task_snapshot(task_id)
            .expect("task exists")
            .sched_priority,
    })
}

/// 将单节点 CPU 0 亲和性 mask 写入 `out`（长度至少为 `cpusetsize` 字节）。
#[inline]
pub fn fill_cpu_affinity_mask(out: &mut [u8]) {
    for byte in out.iter_mut() {
        *byte = 0;
    }
    if !out.is_empty() {
        out[0] |= 1;
    }
}

/// 返回写入 userspace 的有效 mask 字节数。
#[must_use]
#[inline]
pub const fn cpu_affinity_ret_bytes() -> usize {
    SCHED_CPU_MASK_RET_BYTES
}

/// 校验 affinity 查询缓冲区长度。
#[inline]
pub fn validate_cpu_affinity_buf_len(cpusetsize: usize) -> Result<(), SchedError> {
    if cpusetsize < SCHED_CPU_MASK_MIN_BYTES {
        Err(SchedError::InvalidArg)
    } else {
        Ok(())
    }
}

/// 设置调度策略。
#[inline]
pub fn set_scheduler(
    task_id: TaskId,
    policy: SchedPolicy,
    param: SchedParam,
) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    validate_policy_param(policy, param)?;
    match scheduler::apply_sched_policy_change(task_id, policy, param)? {
        SchedPolicyChangeAction::NoReschedule => Ok(()),
        SchedPolicyChangeAction::RescheduleNow => {
            scheduler::suspend_current_and_run_next();
            Ok(())
        }
    }
}

/// 设置调度参数（保持当前 policy 不变）。
#[inline]
pub fn set_param(task_id: TaskId, param: SchedParam) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    let policy = get_scheduler(task_id)?;
    validate_policy_param(policy, param)?;
    let full_param = SchedParam { priority: param.priority };
    match scheduler::apply_sched_policy_change(task_id, policy, full_param)? {
        SchedPolicyChangeAction::NoReschedule => Ok(()),
        SchedPolicyChangeAction::RescheduleNow => {
            scheduler::suspend_current_and_run_next();
            Ok(())
        }
    }
}

/// 设置 CPU 亲和性；单核 bring-up 仅支持 CPU0，mask 包含 CPU0 即成功。
#[inline]
pub fn set_affinity(task_id: TaskId, mask: &[u8]) -> Result<(), SchedError> {
    ensure_task_exists(task_id)?;
    if mask.first().is_some_and(|byte| (byte & 1) != 0) {
        Ok(())
    } else {
        Err(SchedError::InvalidArg)
    }
}

// 确认 task 仍存在于调度器 registry。
#[inline]
fn ensure_task_exists(task_id: TaskId) -> Result<(), SchedError> {
    if scheduler::task_snapshot(task_id).is_some() {
        Ok(())
    } else {
        Err(SchedError::NoSuchTask)
    }
}

// 按策略校验 priority 取值范围。
#[inline]
fn validate_policy_param(policy: SchedPolicy, param: SchedParam) -> Result<(), SchedError> {
    match policy {
        SchedPolicy::Other => {
            if param.priority != 0 {
                Err(SchedError::InvalidArg)
            } else {
                Ok(())
            }
        }
        SchedPolicy::Fifo | SchedPolicy::Rr => {
            if (1..=99).contains(&param.priority) {
                Ok(())
            } else {
                Err(SchedError::InvalidArg)
            }
        }
    }
}
