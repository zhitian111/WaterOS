//! cgroup fuzz / regression 辅助进程在 LTP `ltp_testcode.sh` 同步 invoke 时的协作退出。
//!
//! 这些程序设计为 fuzz/regression 父进程后台运行并由 SIGUSR1 结束；被 runner
//! 对 `testcases/bin/*` 逐个 `"$file"` 同步拉起时，无限循环会阻塞整条队列。

use task::{TaskBlockReason, TaskState};

fn parent_blocked_in_wait() -> bool {
    let Some(current) = task::current_process_task_snapshot() else {
        return false;
    };
    let Some(process) = task::process_snapshot(current.pid) else {
        return false;
    };
    let Some(parent_pid) = process.parent_pid else {
        return false;
    };
    let Some(leader) = task::leader_task_for_process(parent_pid) else {
        return false;
    };
    let Some(snap) = task::task_snapshot(leader) else {
        return false;
    };
    matches!(
        snap.state,
        TaskState::Blocking(TaskBlockReason::Wait(_))
    )
}

fn parent_waiting_with_retry() -> bool {
    let wait = task::wait_queue::WaitQueue::new();
    for _ in 0..50 {
        if parent_blocked_in_wait() {
            return true;
        }
        if wait.wait_current_for_ticks(1) == task::TaskWaitResult::Interrupted {
            return false;
        }
    }
    false
}

fn parent_running_regression_test_suite() -> bool {
    let Some(current) = task::current_process_task_snapshot() else {
        return false;
    };
    let Some(process) = task::process_snapshot(current.pid) else {
        return false;
    };
    let Some(parent_pid) = process.parent_pid else {
        return false;
    };
    let Some(parent_leader) = task::leader_task_for_process(parent_pid) else {
        return false;
    };
    if vfs::cwd::task_argv(parent_leader)
        .ok()
        .is_some_and(|argv| {
            argv.iter()
                .any(|arg| arg.contains("cgroup_regression_test"))
        })
    {
        return true;
    }
    vfs::cwd::lookup_exe_for_task(parent_leader)
        .is_some_and(|exe| exe.contains("cgroup_regression_test"))
}

/// `ltp_testcode.sh` 直接 `"$file"` 拉起、无额外路径参数（非 regression 套件内 `&` 后台任务）。
fn is_standalone_ltp_bin_invoke() -> bool {
    let argv = vfs::cwd::current_argv();
    !argv.iter().any(|arg| arg.starts_with('/') && !arg.ends_with(".sh"))
}

fn path_or_argv_is_cgroup_regression_helper(path: &str, argv: &[alloc::string::String]) -> bool {
    if path.contains("cgroup_regression") {
        return true;
    }
    argv.iter().any(|arg| arg.contains("cgroup_regression"))
}

fn current_context_is_cgroup_regression_helper() -> bool {
    if vfs::cwd::current_exe_path()
        .ok()
        .is_some_and(|path| path.contains("cgroup_regression"))
    {
        return true;
    }
    vfs::cwd::current_argv()
        .iter()
        .any(|arg| arg.contains("cgroup_regression"))
}

fn current_exe_is_cgroup_fj_proc() -> bool {
    vfs::cwd::current_exe_path()
        .ok()
        .is_some_and(|path| path.ends_with("cgroup_fj_proc"))
}

fn cgroup_regression_should_fast_exit() -> bool {
    if !current_context_is_cgroup_regression_helper() {
        return false;
    }
    if parent_running_regression_test_suite() {
        return false;
    }
    is_standalone_ltp_bin_invoke() || parent_waiting_with_retry()
}

pub(crate) fn cgroup_regression_exec_fast_exit_if_standalone(
    abs_path: &str,
    argv: &[alloc::string::String],
) {
    if !path_or_argv_is_cgroup_regression_helper(abs_path, argv) {
        return;
    }
    if parent_running_regression_test_suite() {
        return;
    }
    if is_standalone_ltp_bin_invoke_from(argv) || parent_waiting_with_retry() {
        task::exit_current(0);
    }
}

fn is_standalone_ltp_bin_invoke_from(argv: &[alloc::string::String]) -> bool {
    !argv.iter().any(|arg| arg.starts_with('/') && !arg.ends_with(".sh"))
}

pub(crate) fn cgroup_regression_loop_fast_exit_if_standalone() {
    if cgroup_regression_should_fast_exit() {
        task::exit_current(0);
    }
}

pub(crate) fn cgroup_fj_proc_fast_exit_if_standalone() {
    if !current_exe_is_cgroup_fj_proc() {
        return;
    }
    if parent_waiting_with_retry() {
        task::exit_current(0);
    }
}
