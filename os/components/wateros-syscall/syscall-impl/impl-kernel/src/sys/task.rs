//! 任务相关系统调用：`yield`、`exit`、`waitpid`、`getpid`/`gettid`、
//! `get_time`、`nanosleep`。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

pub(crate) fn sys_yield() -> UserRet {
    task::yield_now();
    UserRet::from_success(0)
}

pub(crate) fn sys_exit(exit_code : isize) -> isize { task::exit_current(exit_code) }

pub(crate) fn sys_get_time() -> UserRet { UserRet::from_success(task::current_tick() as usize) }

pub(crate) fn sys_getpid() -> UserRet {
    task::current_task_id().map(UserRet::from_success)
                           .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

fn write_exit_code(exit_code_ptr : usize, exit_code : isize) {
    if exit_code_ptr != 0 {
        unsafe {
            (exit_code_ptr as *mut i32).write(exit_code as i32);
        }
    }
}

fn finish_wait_result(exited : task::ExitedTask, exit_code_ptr : usize) -> UserRet {
    write_exit_code(exit_code_ptr, exited.exit_code);
    vfs::cwd::drop_task_cwd(exited.id);
    UserRet::from_success(exited.id)
}

/// `waitpid`/`wait4` 早期语义：维护最小父子关系并阻塞等待子任务退出；暂不解析
/// options。
pub(crate) fn sys_waitpid(args : SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let exit_code_ptr = args.arg(1);
    let current_task_id = match task::current_task_id() {
        Some(task_id) => task_id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if pid == -1 {
        loop {
            if let Some(exited) = task::reap_one_exited_child(current_task_id) {
                return finish_wait_result(exited, exit_code_ptr);
            }
            if !task::has_child(current_task_id) {
                return UserRet::from_error(ErrNo::ECHILD);
            }
            task::wait_on(task::TaskWaitHandle::for_child_exit(current_task_id));
        }
    }
    if pid <= 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let task_id = pid as usize;
    match task::task_snapshot(task_id) {
        Some(snapshot) if snapshot.parent_id == Some(current_task_id) => {}
        Some(_) => return UserRet::from_error(ErrNo::ECHILD),
        None => return UserRet::from_error(ErrNo::ECHILD),
    }
    loop {
        if let Some(exited) = task::reap_exited_task(task_id) {
            return finish_wait_result(exited, exit_code_ptr);
        }
        if task::task_snapshot(task_id).is_none() {
            return UserRet::from_error(ErrNo::ECHILD);
        }
        task::wait_for_task_exit(task_id);
    }
}

/// `nanosleep` 临时映射到一个调度
/// tick；真实时间换算待平台频率语义接入后再替换。
pub(crate) fn sys_nanosleep(args : SyscallArgs) -> UserRet {
    let req_ptr = args.arg(0);
    if req_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let req = unsafe { (req_ptr as *const UserTimespec).read() };
    if req.sec < 0 || req.nsec < 0 || req.nsec >= 1_000_000_000 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if req.sec == 0 && req.nsec == 0 {
        return UserRet::from_success(0);
    }
    task::sleep_for_ticks(1);
    UserRet::from_success(0)
}
