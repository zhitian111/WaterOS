//! `execve(2)` — 替换当前进程映像。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use mm::api::executable::ExecResolveError;
use mm::api::kernel_bringup::{
    LoadElfError, LoadProgramError, PrepareUserStackError, RootVolumeReadError,
};
use mm::api::user_access::UserMemoryOps;
use mm::ActiveUserMemoryOps;

use crate::user_copy::copy_user_path_cstr;

/// Linux-compatible upper bound reported for the combined argv/envp payload.
const EXEC_ARG_MAX : usize = 2 * 1024 * 1024;
/// Leave room in the fixed 2 MiB initial user stack for argc and auxv.
const EXEC_STACK_OVERHEAD : usize = 16 * 1024;

// ── 公开入口 ─────────────────────────────────────────────────────

pub(crate) fn sys_execve(args : SyscallArgs) -> UserRet {
    let path_ptr = args.arg(0);
    let argv_ptr = args.arg(1);
    let envp_ptr = args.arg(2);

    match do_execve(path_ptr, argv_ptr, envp_ptr) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn do_execve(path_ptr : usize, argv_ptr : usize, envp_ptr : usize) -> Result<(), ErrNo> {
    let path = copy_user_path_cstr(path_ptr,
                                   crate::user_copy::USER_PATH_MAX)?;
    let abs_path = vfs::cwd::resolve_for_current_task(&path).unwrap_or(path);

    let mut arg_budget = EXEC_ARG_MAX - EXEC_STACK_OVERHEAD;
    let argv = read_string_array(argv_ptr, &mut arg_budget)?;
    let envp = read_string_array(envp_ptr, &mut arg_budget)?;

    let argv_refs : Vec<&str> = argv.iter()
                                    .map(String::as_str)
                                    .collect();
    let loaded_program = match mm::kernel_mm::load_program_from_path(abs_path.as_str(), &argv_refs)
    {
        Ok(program) => program,
        Err(err) => {
            let errno = load_program_to_errno(err);
            return Err(errno);
        }
    };
    let new_elf = loaded_program.elf;
    let final_argv = loaded_program.argv;
    let executable_path = loaded_program.executable_path;
    let final_argv_refs : Vec<&str> = final_argv.iter()
                                                .map(String::as_str)
                                                .collect();
    let envp_refs : Vec<&str> = envp.iter()
                                    .map(String::as_str)
                                    .collect();
    let new_sp = match mm::kernel_mm::prepare_elf_user_stack(&new_elf, &final_argv_refs, &envp_refs)
    {
        Ok(new_sp) => new_sp,
        Err(err) => {
            let errno = prepare_stack_to_errno(err);
            mm::kernel_mm::drop_user_aspace(new_elf.user_aspace_ptr);
            return Err(errno);
        }
    };

    let current_signal_task = match crate::sys::ipc::signal::ensure_current_signal_state() {
        Ok(state) => state.task_id,
        Err(errno) => {
            mm::kernel_mm::drop_user_aspace(new_elf.user_aspace_ptr);
            return Err(errno);
        }
    };
    // Allocate signal state before entering sibling teardown; this is the last
    // preparation step that can fail independently of the thread group.
    crate::sys::ipc::robust::robust_exit_cleanup_siblings_for_exec();
    let terminated = match task::terminate_other_threads_for_exec() {
        Ok(terminated) => terminated,
        Err(_) => {
            mm::kernel_mm::drop_user_aspace(new_elf.user_aspace_ptr);
            return Err(ErrNo::EINVAL);
        }
    };
    crate::sys::ipc::signal::deliver_parent_death_notifications(
        terminated.parent_death_notifications.iter().copied());
    let killed_threads = terminated.exited_tasks;

    let (argc, argv_ptr, envp_ptr) = initial_entry_args(new_sp, final_argv_refs.len());

    crate::sys::ipc::signal::on_exec(current_signal_task, &killed_threads);

    let old_aspace = task::current_task_user_aspace_ptr();
    let vfork_child = super::vfork::current_is_child();
    let current_tid = task::current_task_id().expect("execve requires a current task");
    super::super::shm::drop_task_attachments(current_tid, old_aspace);
    for exited in &killed_threads {
        super::wait::drop_task_runtime_resources_with_aspace(exited.id, old_aspace);
    }
    if !vfork_child {
        mm::kernel_mm::drop_user_aspace(old_aspace);
    }

    // 越过不可回退点：后续操作不可传播错误，否则进程将处于无地址空间的死亡状态。
    // 只能 log 警告后继续，确保 execve 最终一定成功。
    match vfs::fd::close_cloexec_fds_for_current_task() {
        Ok((closed_fds, terminal_ids)) => {
            for fd in closed_fds {
                crate::unix_sock::unregister(current_tid, fd);
            }
            crate::sys::fs::close::dispatch_terminal_events(&terminal_ids);
        }
        Err(e) => {
            log::warn!("[execve] close_cloexec_fds failed (continuing): {:?}",
                       e);
        }
    }

    // TODO(cred-exec-setuid): 可执行文件 S_ISUID/S_ISGID 应在 cred::on_exec 内更新凭证。
    cred::on_exec(current_tid);
    if let Err(e) = vfs::cwd::set_task_exe_path(current_tid, executable_path.as_str()) {
        log::warn!("[execve] set_task_exe_path failed (continuing): {:?}",
                   e);
    }
    let _ = vfs::cwd::set_task_argv(current_tid,
                                    final_argv.iter()
                                              .map(String::as_str));
    let mut comm = [0u8; 16];
    let basename = executable_path.rsplit('/')
                                  .find(|part| !part.is_empty())
                                  .unwrap_or("");
    let len = core::cmp::min(15, basename.len());
    comm[..len].copy_from_slice(&basename.as_bytes()[..len]);
    let _ = task::set_thread_comm(current_tid, comm);

    let image_info = task::UserImageInfo::new(new_elf.image_base, new_elf.image_size);
    let stack_info = task::UserStack::from_range(new_elf.stack_bottom, new_elf.stack_top);
    task::execve_current(new_elf.entry_pc,
                         new_sp,
                         argc,
                         argv_ptr,
                         envp_ptr,
                         new_elf.satp,
                         new_elf.user_aspace_ptr,
                         image_info,
                         stack_info);
    if vfork_child {
        super::vfork::complete_current();
    }

    Ok(())
}

fn initial_entry_args(sp : usize, argc : usize) -> (usize, usize, usize) {
    let word = core::mem::size_of::<usize>();
    let argv = sp + word;
    let envp = argv + (argc + 1) * word;
    (argc, argv, envp)
}

fn prepare_stack_to_errno(e : PrepareUserStackError) -> ErrNo {
    match e {
        PrepareUserStackError::StackOverflow => ErrNo::E2BIG,
        PrepareUserStackError::AccessViolation | PrepareUserStackError::NoUserAspace => {
            ErrNo::EFAULT
        }
    }
}

fn load_program_to_errno(e : LoadProgramError) -> ErrNo {
    match e {
        LoadProgramError::Script(ExecResolveError::NotExecutable) => ErrNo::ENOEXEC,
        LoadProgramError::Script(ExecResolveError::InvalidShebang) |
        LoadProgramError::Script(ExecResolveError::RecursionLimit) => ErrNo::EINVAL,
        LoadProgramError::Elf(elf_err) => load_elf_to_errno(elf_err),
    }
}

fn load_elf_to_errno(e : LoadElfError) -> ErrNo {
    use mm::api::error::MmError;

    match e {
        LoadElfError::NoRootFs => ErrNo::ENOENT,
        LoadElfError::RootVolume(r) => root_volume_to_errno(r),
        LoadElfError::Mm(MmError::OutOfMemory) => ErrNo::ENOMEM,
        LoadElfError::Mm(_) => ErrNo::EFAULT,
        LoadElfError::TooSmall |
        LoadElfError::BadMagic |
        LoadElfError::BadClass |
        LoadElfError::BadEndian |
        LoadElfError::BadMachine |
        LoadElfError::Parse => ErrNo::ENOEXEC,
    }
}

fn root_volume_to_errno(e : RootVolumeReadError) -> ErrNo {
    match e {
        RootVolumeReadError::NotFound => ErrNo::ENOENT,
        RootVolumeReadError::NotAFile => ErrNo::EACCES,
        RootVolumeReadError::NotMounted |
        RootVolumeReadError::InvalidPath |
        RootVolumeReadError::NotUtf8 |
        RootVolumeReadError::Unsupported |
        RootVolumeReadError::Driver |
        RootVolumeReadError::Corrupt |
        RootVolumeReadError::Io => ErrNo::EIO,
    }
}

/// 从用户态 `char **` 数组读取字符串，并从 argv/envp 共享预算中扣除字符串和指针。
fn read_string_array(array_ptr : usize, budget : &mut usize) -> Result<Vec<String>, ErrNo> {
    let mut result = Vec::new();
    if array_ptr == 0 {
        return Ok(result);
    }
    let ops = ActiveUserMemoryOps::new(task::current_task_user_aspace_ptr());
    let word_size = core::mem::size_of::<usize>();
    let mut ptr_bytes = [0u8; core::mem::size_of::<usize>()];
    loop {
        let offset = result.len()
                           .checked_mul(word_size)
                           .and_then(|offset| array_ptr.checked_add(offset))
                           .ok_or(ErrNo::EFAULT)?;
        if *budget < word_size {
            return Err(ErrNo::E2BIG);
        }
        if ops.copy_from_user(&mut ptr_bytes,
                              mm::api::addr::VirtAddr(offset))
              .is_err()
        {
            return Err(ErrNo::EFAULT);
        }
        *budget -= word_size;
        let ptr = usize::from_le_bytes(ptr_bytes);
        if ptr == 0 {
            break;
        }
        let max_len = crate::user_copy::USER_PATH_MAX.min(*budget);
        if max_len == 0 {
            return Err(ErrNo::E2BIG);
        }
        let value = copy_user_path_cstr(ptr, max_len).map_err(|error| {
                                                         if error == ErrNo::ENAMETOOLONG {
                                                             ErrNo::E2BIG
                                                         } else {
                                                             error
                                                         }
                                                     })?;
        *budget = budget.checked_sub(value.len() + 1)
                        .ok_or(ErrNo::E2BIG)?;
        result.push(value);
    }
    Ok(result)
}
