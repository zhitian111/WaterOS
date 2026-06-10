//! 调度策略与 CPU 亲和性 **原语**。

use api_v0::{
    SchedError, SchedParam, SchedPolicy, SCHED_CPU_MASK_MIN_BYTES, SCHED_CPU_MASK_RET_BYTES,
    TaskId,
};

use crate::scheduler::{self, SchedPolicyChangeAction};

/// 将 Linux `pid`（0 = 当前线程）解析为 [`TaskId`]。
pub fn resolve_sched_pid(pid: isize) -> Result<TaskId, SchedError> {
    if pid == 0 {
        return scheduler::current_task_id().ok_or(SchedError::NoSuchTask);
    }
    if pid < 0 {
        return Err(SchedError::InvalidArg);
    }
    let task_id = pid as TaskId;
    if scheduler::task_snapshot(task_id).is_some() {
        Ok(task_id)
    } else {
        Err(SchedError::NoSuchTask)
    }
}

/// 查询任务的有效调度策略。
pub fn get_scheduler(task_id: TaskId) -> Result<SchedPolicy, SchedError> {
    ensure_task_exists(task_id)?;
    Ok(scheduler::task_snapshot(task_id)
        .expect("task exists")
        .sched_policy)
}

/// 查询任务的调度参数。
pub fn get_param(task_id: TaskId) -> Result<SchedParam, SchedError> {
    ensure_task_exists(task_id)?;
    Ok(SchedParam {
        priority: scheduler::task_snapshot(task_id)
            .expect("task exists")
            .sched_priority,
    })
}

/// 将单节点 CPU 0 亲和性 mask 写入 `out`（长度至少为 `cpusetsize` 字节）。
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
pub const fn cpu_affinity_ret_bytes() -> usize {
    SCHED_CPU_MASK_RET_BYTES
}

/// 校验 affinity 查询缓冲区长度。
pub fn validate_cpu_affinity_buf_len(cpusetsize: usize) -> Result<(), SchedError> {
    if cpusetsize < SCHED_CPU_MASK_MIN_BYTES {
        Err(SchedError::InvalidArg)
    } else {
        Ok(())
    }
}

/// 设置调度策略。
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

/// 设置 CPU 亲和性；bring-up 未实现，恒返回 [`SchedError::NotPermitted`]。
pub fn set_affinity(_task_id: TaskId, _mask: &[u8]) -> Result<(), SchedError> {
    Err(SchedError::NotPermitted)
}

fn ensure_task_exists(task_id: TaskId) -> Result<(), SchedError> {
    if scheduler::task_snapshot(task_id).is_some() {
        Ok(())
    } else {
        Err(SchedError::NoSuchTask)
    }
}

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
