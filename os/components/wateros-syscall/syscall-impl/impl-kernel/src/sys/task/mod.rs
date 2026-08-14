pub(crate) mod clone;
pub(crate) mod execve;
pub(crate) mod priority;
pub(crate) mod personality;
pub(crate) mod process;
pub(crate) mod rlimit;
pub(crate) mod rseq;
pub(crate) mod sched;
pub(crate) mod task;
pub(crate) mod unshare;
pub(crate) mod vfork;
pub(crate) mod wait;

pub(crate) use clone::{sys_clone, sys_clone3};
pub(crate) use execve::sys_execve;
pub(crate) use priority::{sys_getpriority, sys_setpriority};
pub(crate) use personality::sys_personality;
pub(crate) use process::{
    sys_getpgid, sys_getpid, sys_getppid, sys_getsid, sys_gettid, sys_set_tid_address, sys_setpgid,
    sys_setsid,
};
pub(crate) use rlimit::{current_umask, sys_getrlimit, sys_prlimit64, sys_setrlimit, sys_umask};
pub(crate) use rseq::sys_rseq;
pub(crate) use sched::{
    sys_getcpu, sys_sched_get_priority_max, sys_sched_get_priority_min, sys_sched_getaffinity,
    sys_sched_getattr, sys_sched_getparam, sys_sched_getscheduler, sys_sched_setaffinity,
    sys_sched_rr_get_interval, sys_sched_setattr, sys_sched_setparam, sys_sched_setscheduler,
};
pub(crate) use task::{
    exit_current_with_wait_code, exit_group_with_wait_code, sys_exit, sys_exit_group, sys_prctl,
    sys_yield, timer_slack_for_task,
};
pub(crate) use unshare::sys_unshare;
pub(crate) use wait::{
    drop_reaped_task_runtime_resources, drop_task_runtime_resources,
    reap_exited_member_threads_runtime_resources, signal_terminate_exit_code,
    wake_clear_child_tid_for_task, sys_waitid, sys_waitpid,
};

#[cfg(feature = "self_test")]
pub(crate) fn self_test() { personality::self_test(); }
