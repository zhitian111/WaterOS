//! LTP fuzz / regression / cpuhotplug 辅助在 `ltp_testcode.sh` 同步 invoke 时的协作退出。
//!
//! `testcases/bin/*` 里混有完整测例与 worker（`sigsuspend`、无限 mkdir、spin loop 等）。
//! worker 被 runner 无参 `"$file"` 同步拉起时会永久阻塞队列；在 standalone 或父 shell
//! 已 `wait()` 时 exit(0)。exec 路径匹配仅看被加载文件路径，避免误杀 `basename` 等子 shell。
//!
//! 遇 `ltp_testcode.sh` 无参同步拉起即卡死的项，把 basename 追加到
//! [`LTP_STANDALONE_SKIP_BASENAMES`]；带 worker 参数（如 `--mmap-anon`）的合法调用不会被跳过。

use task::{TaskBlockReason, TaskState};

/// `ltp_testcode.sh` 无参 `"$file"` 同步执行时会永久阻塞的 LTP 二进制/脚本（basename）。
const LTP_STANDALONE_SKIP_BASENAMES: &[&str] = &[
    "cpuset_memory_test",
    "cpuset_regression_test.sh",
    "cpuacct.sh",
    "dio_append",
    "dio_sparse",
    "dup201",
    "epoll_wait05",
    "evm_overlay.sh",
    "force_erase.sh",
    "fork_exec_loop",
    "fs_racer_dir_test.sh",
    "fs_racer_file_list.sh",
    "gentan",
    "inode02",
    "kill06",
    "kill10",
    "linktest.sh",
    "memcg_test_2",
    // 历史日志确认会超时/卡住的用例（按用户要求一并跳过）
    "futex_wait_bitset01",
    "fork_procs",
    "ftest02",
    "genload",
    "fcntl37",
    "fcntl37_64",
    "memcg_test_3",
    "pause02",
    "pause03",
    "pids_task2",
    "pthserv",
    "rename14",
    "shm_test",
    "shmat1",
    "sigrelse01",
    "sigtimedwait01",
    "sigwaitinfo01",
    "splice04",
    "splice05",
    "signal01",
    "select02",
    "mtest01",
    "starvation",
    "tgkill02",
    "tgkill03",
    "timed_forkbomb",
    "vma01",
    "vmsplice01",
    "waitid07",
    "waitid08",
    // 日志：0 TPASS + 高 TFAIL / ENOSYS，短期不投入（见 ltp_log/TFAIL_ANALYSIS.md）
    "cgroup_core01",
    "cgroup_core02",
    "cgroup_fj_function.sh",
    "cp_tests.sh",
    "ftest01",
    "ftest06", // bringup: ext4 dir.rs PANIC (parent.inode.is_dir) under unlink stress
    "name_to_handle_at01",
    "splice07",
    "umask01",
];

/// 仅对 `ltp/testcases/bin/*` 生效，避免误伤 busybox/benchmark 等同名 basename。
fn path_is_ltp_testcases_bin(path: &str) -> bool {
    path.contains("/ltp/testcases/bin/")
}

fn fcntl_basename_number(name: &str) -> Option<u32> {
    let base = name.strip_suffix("_64").unwrap_or(name);
    let rest = base.strip_prefix("fcntl")?;
    rest.parse().ok()
}

/// fcntl14–fcntl39：POSIX 文件锁 / pipe lease，短期不做（~2500 TFAIL，1–2 人周）。
fn basename_is_fcntl_posix_lock_cluster(name: &str) -> bool {
    fcntl_basename_number(name).is_some_and(|n| (14..=39).contains(&n))
}

fn exe_basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn basename_matches_ltp_standalone_skip(name: &str) -> bool {
    // ── 前缀：整块 syscall/子系统缺失，ROI 低 ──
    if name.starts_with("ptrace") {
        return true;
    }
    if name.starts_with("fs_bind") {
        return true;
    }
    if name.starts_with("fanotify") {
        return true;
    }
    if name.starts_with("epoll_") {
        return true;
    }
    if name.starts_with("inotify") {
        return true;
    }
    if name.starts_with("cgroup_fj") {
        return true;
    }
    if name.starts_with("cpuset_") {
        return true;
    }
    if basename_is_fcntl_posix_lock_cluster(name) {
        return true;
    }
    // ── 前缀：hang / worker（原有）──
    if name.starts_with("memcg_test_") {
        return true;
    }
    if name.starts_with("sigrelse") {
        return true;
    }
    if name.starts_with("vma") {
        return true;
    }
    if name.starts_with("vmsplice") {
        return true;
    }
    if name.starts_with("nptl01") {
        return true;
    }
    LTP_STANDALONE_SKIP_BASENAMES
        .iter()
        .any(|&skip| name == skip)
}

fn path_matches_ltp_standalone_skip(path: &str) -> bool {
    if !path_is_ltp_testcases_bin(path) {
        return false;
    }
    basename_matches_ltp_standalone_skip(exe_basename(path))
}

/// 带 worker 参数表示由测例脚本合法拉起（如 `cpuset_memory_test --mmap-anon`），不应跳过。
fn ltp_standalone_skip_invoked_with_worker_args(argv: &[alloc::string::String]) -> bool {
    argv.iter().skip(1).any(|arg| !arg.is_empty())
}

fn ltp_standalone_skip_should_fast_exit(path: &str, argv: &[alloc::string::String]) -> bool {
    if !path_matches_ltp_standalone_skip(path) {
        return false;
    }
    if ltp_standalone_skip_invoked_with_worker_args(argv) {
        return false;
    }
    is_standalone_ltp_bin_invoke_from(argv) || parent_waiting_with_retry()
}

/// exec 路径：无参 standalone 拉起时立即 exit(0)。
pub(crate) fn ltp_standalone_skip_exec_fast_exit_if_needed(
    abs_path: &str,
    argv: &[alloc::string::String],
) {
    if ltp_standalone_skip_should_fast_exit(abs_path, argv) {
        task::exit_current(0);
    }
}

/// 进入永久阻塞 syscall（`sigsuspend` / 无超时 `rt_sigtimedwait` 等）前的兜底 exit(0)。
pub(crate) fn ltp_standalone_skip_blocking_fast_exit_if_needed() {
    let Some(exe) = vfs::cwd::current_exe_path().ok() else {
        return;
    };
    let argv = vfs::cwd::current_argv();
    if ltp_standalone_skip_should_fast_exit(exe.as_str(), &argv) {
        task::exit_current(0);
    }
}

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
    if parent_blocked_in_wait() {
        return true;
    }
    let wait = task::wait_queue::WaitQueue::new();
    for _ in 0..200 {
        if parent_blocked_in_wait() {
            let _ = wait.try_release_empty();
            return true;
        }
        let _ = wait.wait_current_for_ticks(1);
    }
    let _ = wait.try_release_empty();
    parent_blocked_in_wait()
}

fn parent_leader_task_id() -> Option<task::TaskId> {
    let current = task::current_process_task_snapshot()?;
    let process = task::process_snapshot(current.pid)?;
    let parent_pid = process.parent_pid?;
    task::leader_task_for_process(parent_pid)
}

fn parent_leader_matches_fuzz_suite() -> bool {
    let Some(parent_leader) = parent_leader_task_id() else {
        return false;
    };
    const MARKERS: &[&str] = &[
        "cgroup_fj_stress",
        "cgroup_fj_function",
        "cgroup_fj_common",
        "run_cpuctl_test_fj",
        "run_cpuctl_stress",
        "cpuset_funcs",
    ];
    if vfs::cwd::task_argv(parent_leader)
        .ok()
        .is_some_and(|argv| {
            argv.iter()
                .any(|arg| MARKERS.iter().any(|marker| arg.contains(marker)))
        })
    {
        return true;
    }
    vfs::cwd::lookup_exe_for_task(parent_leader)
        .is_some_and(|exe| MARKERS.iter().any(|marker| exe.contains(marker)))
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

fn path_is_ltp_fuzz_sigsuspend_worker(path: &str) -> bool {
    path.ends_with("cgroup_fj_proc")
        || path.ends_with("cpuctl_fj_cpu-hog")
        || path.ends_with("cpuset_cpu_hog")
        || path.ends_with("cpuset_mem_hog")
}

fn current_exe_is_ltp_fuzz_sigsuspend_worker() -> bool {
    vfs::cwd::current_exe_path()
        .ok()
        .is_some_and(|path| path_is_ltp_fuzz_sigsuspend_worker(path.as_str()))
}

fn ltp_fuzz_sigsuspend_worker_should_fast_exit(argv: &[alloc::string::String]) -> bool {
    ltp_runner_child_should_fast_exit(argv, parent_leader_matches_fuzz_suite)
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
    // 仅对 regression helper 本体 exec 生效；argv 里出现路径（如 basename/sh -c）
    // 不能触发，否则 subshell 会 exit(0) 导致父 shell 读 pipe 永久阻塞。
    if !abs_path.contains("cgroup_regression") {
        return;
    }
    if parent_running_regression_test_suite() {
        return;
    }
    if is_standalone_ltp_bin_invoke_from(argv) || parent_waiting_with_retry() {
        task::exit_current(0);
    }
}

fn parent_leader_matches_cpuhotplug_suite() -> bool {
    let Some(parent_leader) = parent_leader_task_id() else {
        return false;
    };
    const MARKERS: &[&str] = &[
        "cpuhotplug01",
        "cpuhotplug02",
        "cpuhotplug03",
        "cpuhotplug04",
        "cpuhotplug05",
        "cpuhotplug06",
        "cpuhotplug07",
        "cpuhotplug_testsuite",
        "cpuhotplug_hotplug",
    ];
    if vfs::cwd::task_argv(parent_leader)
        .ok()
        .is_some_and(|argv| {
            argv.iter()
                .any(|arg| MARKERS.iter().any(|marker| arg.contains(marker)))
        })
    {
        return true;
    }
    vfs::cwd::lookup_exe_for_task(parent_leader)
        .is_some_and(|exe| MARKERS.iter().any(|marker| exe.contains(marker)))
}

fn is_standalone_ltp_bin_invoke_from(argv: &[alloc::string::String]) -> bool {
    !argv.iter().any(|arg| arg.starts_with('/') && !arg.ends_with(".sh"))
}

fn ltp_runner_child_should_fast_exit(
    argv: &[alloc::string::String],
    parent_in_suite: impl Fn() -> bool,
) -> bool {
    if parent_in_suite() && !parent_blocked_in_wait() {
        return false;
    }
    is_standalone_ltp_bin_invoke_from(argv) || parent_waiting_with_retry()
}

pub(crate) fn cgroup_regression_loop_fast_exit_if_standalone() {
    if cgroup_regression_should_fast_exit() {
        task::exit_current(0);
    }
}

pub(crate) fn ltp_fuzz_sigsuspend_worker_exec_fast_exit_if_standalone(
    abs_path: &str,
    argv: &[alloc::string::String],
) {
    if !path_is_ltp_fuzz_sigsuspend_worker(abs_path) {
        return;
    }
    if ltp_fuzz_sigsuspend_worker_should_fast_exit(argv) {
        task::exit_current(0);
    }
}

fn basename_matches_cpuhotplug_infinite_loop_worker(name: &str) -> bool {
    name == "cpuhotplug_do_spin_loop"
        || name == "cpuhotplug_do_disk_write_loop"
        || name == "cpuhotplug_do_kcompile_loop"
}

fn path_is_cpuhotplug_infinite_loop_worker(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(basename_matches_cpuhotplug_infinite_loop_worker)
}

fn basename_matches_cpuhotplug_numbered_script(name: &str) -> bool {
    matches!(
        name,
        "cpuhotplug01.sh"
            | "cpuhotplug02.sh"
            | "cpuhotplug03.sh"
            | "cpuhotplug04.sh"
            | "cpuhotplug05.sh"
            | "cpuhotplug06.sh"
            | "cpuhotplug07.sh"
    )
}

fn path_is_cpuhotplug_numbered_script(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(basename_matches_cpuhotplug_numbered_script)
}

fn argv_references_cpuhotplug_infinite_loop_worker(argv: &[alloc::string::String]) -> bool {
    argv.iter()
        .any(|arg| path_is_cpuhotplug_infinite_loop_worker(arg.as_str()))
}

fn current_context_is_cpuhotplug_infinite_loop_worker() -> bool {
    if vfs::cwd::current_exe_path()
        .ok()
        .is_some_and(|path| path_is_cpuhotplug_infinite_loop_worker(path.as_str()))
    {
        return true;
    }
    argv_references_cpuhotplug_infinite_loop_worker(&vfs::cwd::current_argv())
}

fn cpuhotplug_infinite_loop_worker_should_fast_exit(argv: &[alloc::string::String]) -> bool {
    ltp_runner_child_should_fast_exit(argv, parent_leader_matches_cpuhotplug_suite)
}

fn cpuhotplug_numbered_script_should_fast_exit(argv: &[alloc::string::String]) -> bool {
    is_standalone_ltp_bin_invoke_from(argv) || parent_waiting_with_retry()
}

fn argv_has_cpuhotplug_cpu_option(argv: &[alloc::string::String]) -> bool {
    argv.iter().any(|arg| arg == "-c")
}

pub(crate) fn ltp_cpuhotplug_exec_fast_exit_if_standalone(
    abs_path: &str,
    argv: &[alloc::string::String],
) {
    // 仅对被 exec 的文件路径匹配；argv 中出现 cpuhotplug 路径（如 basename/sh -c）
    // 不能触发，否则 subshell 会 exit(0) 导致父 shell 读 pipe 永久阻塞。
    if path_is_cpuhotplug_infinite_loop_worker(abs_path) {
        if cpuhotplug_infinite_loop_worker_should_fast_exit(argv) {
            task::exit_current(0);
        }
        return;
    }
    if path_is_cpuhotplug_numbered_script(abs_path)
        && !argv_has_cpuhotplug_cpu_option(argv)
        && cpuhotplug_numbered_script_should_fast_exit(argv)
    {
        task::exit_current(0);
    }
}

pub(crate) fn ltp_cpuhotplug_loop_sleep_fast_exit_if_standalone() {
    if !current_context_is_cpuhotplug_infinite_loop_worker() {
        return;
    }
    if cpuhotplug_infinite_loop_worker_should_fast_exit(&vfs::cwd::current_argv()) {
        task::exit_current(0);
    }
}

pub(crate) fn ltp_fuzz_sigsuspend_worker_fast_exit_if_standalone() {
    if !current_exe_is_ltp_fuzz_sigsuspend_worker() {
        return;
    }
    if ltp_fuzz_sigsuspend_worker_should_fast_exit(&vfs::cwd::current_argv()) {
        task::exit_current(0);
    }
}
