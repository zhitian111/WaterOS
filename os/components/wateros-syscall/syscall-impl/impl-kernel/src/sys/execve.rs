//! `execve(2)` — 替换当前进程映像。
//!
//! 加载新 ELF、销毁旧地址空间、构造用户栈（argv/envp/auxv）、关闭 CLOEXEC fd，
//! 最后更新 TCB 使当前任务跳转到新程序入口。

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use mm::api::kernel_bringup::PrepareUserStackError;
use mm::api::user_access::UserMemoryOps;
use mm::ActiveUserMemoryOps;

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

// ── 公开入口 ─────────────────────────────────────────────────────

pub(crate) fn sys_execve(args: SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let argv_ptr = args.arg(1);
    let envp_ptr = args.arg(2);

    match do_execve(path_ptr, argv_ptr, envp_ptr) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn do_execve(path_ptr: usize, argv_ptr: usize, envp_ptr: usize) -> Result<(), ErrNo> {
    let path = copy_user_path_cstr(path_ptr, 256)?;
    let abs_path = vfs::cwd::resolve_for_current_task(&path).unwrap_or(path);

    let argv = read_string_array(argv_ptr)?;
    let envp = read_string_array(envp_ptr)?;

    super::robust::robust_exit_cleanup_siblings_for_exec();
    let killed_threads = task::terminate_other_threads_for_exec().map_err(|_| ErrNo::EINVAL)?;

    let new_elf = mm::kernel_mm::from_elf_path(&abs_path).map_err(|_| ErrNo::ENOENT)?;

    let argv_refs: Vec<&str> = argv
        .iter()
        .map(String::as_str)
        .collect();
    let envp_refs: Vec<&str> = envp
        .iter()
        .map(String::as_str)
        .collect();
    let new_sp = mm::kernel_mm::prepare_elf_user_stack(&new_elf, &argv_refs, &envp_refs)
        .map_err(prepare_stack_to_errno)?;
    let (argc, argv_ptr, envp_ptr) = initial_entry_args(new_sp, argv_refs.len());

    for exited in &killed_threads {
        vfs::cwd::drop_task_cwd(exited.id);
        vfs::fd::drop_task_fd_table(exited.id);
        cred::drop_task_cred(exited.id);
    }

    let old_aspace = task::current_task_user_aspace_ptr();
    mm::kernel_mm::drop_user_aspace(old_aspace);

    vfs::fd::close_cloexec_fds_for_current_task().map_err(vfs_error_to_errno)?;

    let current_tid = task::current_task_id().expect("execve requires a current task");
    // TODO(cred-exec-setuid): 可执行文件 S_ISUID/S_ISGID 应在 cred::on_exec 内更新凭证。
    cred::on_exec(current_tid);
    vfs::cwd::set_task_exe_path(current_tid, abs_path.as_str()).map_err(vfs_error_to_errno)?;

    let image_info = task::UserImageInfo::new(new_elf.image_base, new_elf.image_size);
    let stack_info = task::UserStack::from_range(new_elf.stack_bottom, new_elf.stack_top);
    task::execve_current(
        new_elf.entry_pc,
        new_sp,
        argc,
        argv_ptr,
        envp_ptr,
        new_elf.satp,
        new_elf.user_aspace_ptr,
        image_info,
        stack_info,
    );

    Ok(())
}

fn initial_entry_args(sp: usize, argc: usize) -> (usize, usize, usize) {
    let word = core::mem::size_of::<usize>();
    let argv = sp + word;
    let envp = argv + (argc + 1) * word;
    (argc, argv, envp)
}

fn prepare_stack_to_errno(e: PrepareUserStackError) -> ErrNo {
    match e {
        PrepareUserStackError::StackOverflow
        | PrepareUserStackError::AccessViolation
        | PrepareUserStackError::NoUserAspace => ErrNo::EFAULT,
    }
}

/// 从用户态 `char **` 数组读取所有字符串。
fn read_string_array(array_ptr: usize) -> Result<Vec<String>, ErrNo> {
    let mut result = Vec::new();
    if array_ptr == 0 {
        return Ok(result);
    }
    let ops = ActiveUserMemoryOps::new(task::current_task_user_aspace_ptr());
    let mut ptr_size = [0u8; 8];
    loop {
        if ops
            .copy_from_user(
                &mut ptr_size,
                mm::api::addr::VirtAddr(array_ptr + result.len() * 8),
            )
            .is_err()
        {
            return Ok(result);
        }
        let ptr = usize::from_le_bytes(ptr_size);
        if ptr == 0 {
            break;
        }
        match copy_user_path_cstr(ptr, 256) {
            Ok(s) => result.push(s),
            Err(_) => break,
        }
    }
    Ok(result)
}
