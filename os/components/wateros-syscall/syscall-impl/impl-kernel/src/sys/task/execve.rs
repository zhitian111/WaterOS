//! `execve(2)` — 替换当前进程映像。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials, Uid};
use mm::api::executable::ExecResolveError;
use mm::api::kernel_bringup::{
    LoadElfError, LoadProgramError, PrepareUserStackError, RootVolumeReadError,
};
use mm::api::user_access::UserMemoryOps;
use mm::ActiveUserMemoryOps;
use vfs::api::{FinalSymlink, VfsError, VfsNodeType};
use vfs::SingleRootReadView;

use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

/// Linux-compatible upper bound reported for the combined argv/envp payload.
const EXEC_ARG_MAX : usize = 2 * 1024 * 1024;
/// Leave room in the fixed 2 MiB initial user stack for argc and auxv.
const EXEC_STACK_OVERHEAD : usize = 16 * 1024;
/// Linux auxv tags needed by the nested-QEMU capability diagnostic.
const AT_NULL : usize = 0;
const AT_HWCAP : usize = 16;
const AUXV_DIAGNOSTIC_MAX_PAIRS : usize = 64;

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
    // execve 和其他 pathname syscall 一样，必须先完成 NAME_MAX、路径前缀和
    // 搜索权限检查。直接把字符串交给 ELF loader 会把这些错误折叠成
    // ENOENT/EIO/ENOEXEC，破坏 Linux 规定的 errno 优先级。
    let abs_path = preflight_executable_path(path.as_str())?;

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

    if let Some(qemu_path) = qemu_system_invocation(executable_path.as_str(),
                                                    &final_argv_refs)
    {
        log_qemu_host_capability(&new_elf,
                                 qemu_path,
                                 new_sp,
                                 final_argv_refs.len(),
                                 envp_refs.len());
    }

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

    // S_ISUID/S_ISGID：exec 时把有效 uid/gid 换成文件属主（su/sudo/mount 等
    // setuid-root 程序依赖）。Linux 同步更新 saved；WaterOS 简化模型只动
    // effective/saved（real 保持不变）。
    if let Ok(meta) = vfs::active_impl::backend().metadata(executable_path.as_str()) {
        let mode = u32::from(meta.mode);
        if mode & 0o4000 != 0 {
            let file_uid = Uid(meta.uid);
            cred::set_resuid(None, Some(file_uid), Some(file_uid));
        }
        if mode & 0o2000 != 0 {
            let file_gid = Gid(meta.gid);
            cred::set_resgid(None, Some(file_gid), Some(file_gid));
        }
        // 提权到 root 时同步能力（WaterOS 简化模型：root 具备 ROOT caps）。
        if mode & 0o4000 != 0 && meta.uid == 0 {
            if let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) {
                let _ = task::set_process_caps(pid, task::ProcessCaps::ROOT);
            }
        }
    }
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

fn preflight_executable_path(path : &str) -> Result<String, ErrNo> {
    let resolved = super::super::fs::path_at::resolve_path_at(super::super::fs::path_at::AT_FDCWD,
                                                              path)?;
    // 先展开中间符号链接（/bin -> usr/bin、/bin/sh -> dash）再检查父目录
    // 权限；否则非 root 进程 exec 经 symlink 的路径时，会把 /bin 这类链接
    // 误报为 ENOTDIR（su 切用户后 exec /bin/sh 即触发；root 因检查短路
    // 一直未暴露）。
    let resolved =
        super::super::fs::path_at::resolve_symlinks(resolved.as_str(), FinalSymlink::Follow)?;
    let credentials = cred::current_credentials();
    check_parent_search(resolved.as_str(), &credentials)?;
    let metadata = vfs::active_impl::backend().metadata(resolved.as_str())
                                              .map_err(|error| match error {
                                                  VfsError::NotAFile => ErrNo::ENOTDIR,
                                                  other => vfs_error_to_errno(other),
                                              })?;
    if metadata.node_type != VfsNodeType::File ||
       !may_execute(metadata.mode,
                    metadata.uid,
                    metadata.gid,
                    &credentials)
    {
        return Err(ErrNo::EACCES);
    }
    Ok(resolved)
}

fn check_parent_search(path : &str, credentials : &ProcessCredentials) -> Result<(), ErrNo> {
    if credentials.effective_uid
                  .0 ==
       0
    {
        return Ok(());
    }
    let components : Vec<&str> = path.trim_start_matches('/')
                                     .split('/')
                                     .filter(|component| !component.is_empty())
                                     .collect();
    let mut current = String::from("/");
    for component in components.iter()
                               .take(components.len()
                                               .saturating_sub(1))
    {
        if current != "/" {
            current.push('/');
        }
        current.push_str(component);
        let metadata = vfs::active_impl::backend().metadata(current.as_str())
                                                  .map_err(vfs_error_to_errno)?;
        if metadata.node_type != VfsNodeType::Directory {
            return Err(ErrNo::ENOTDIR);
        }
        if !may_execute(metadata.mode,
                        metadata.uid,
                        metadata.gid,
                        credentials)
        {
            return Err(ErrNo::EACCES);
        }
    }
    Ok(())
}

fn may_execute(mode : u16,
               owner_uid : u32,
               owner_gid : u32,
               credentials : &ProcessCredentials)
               -> bool {
    let mode = u32::from(mode);
    if credentials.effective_uid
                  .0 ==
       0
    {
        return mode & 0o111 != 0;
    }
    let bit = if credentials.effective_uid
                            .0 ==
                 owner_uid
    {
        0o100
    } else if credentials.effective_gid == Gid(owner_gid) ||
              credentials.supplementary_groups
                         .iter()
                         .take(credentials.supplementary_group_len)
                         .any(|group| *group == Gid(owner_gid))
    {
        0o010
    } else {
        0o001
    };
    mode & bit != 0
}

fn is_qemu_system_executable(path : &str) -> bool {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .is_some_and(|name| name.starts_with("qemu-system-"))
}

/// Return the nested QEMU path for both direct execution and the image-bundled
/// `ld-linux --library-path ... qemu-system-*` form used by online BuildStorm.
fn qemu_system_invocation<'a>(executable_path : &'a str, argv : &'a [&'a str]) -> Option<&'a str> {
    if is_qemu_system_executable(executable_path) {
        return Some(executable_path);
    }
    argv.iter()
        .copied()
        .find(|argument| is_qemu_system_executable(argument))
}

/// Read back the auxv bytes from the newly prepared address space.  This checks
/// what the QEMU process will actually receive instead of merely logging the
/// constant used while constructing the stack.
fn log_qemu_host_capability(elf : &mm::api::kernel_bringup::LoadedElf,
                            path : &str,
                            sp : usize,
                            argc : usize,
                            envc : usize) {
    let word = core::mem::size_of::<usize>();
    let words_before_auxv = 1usize.checked_add(argc)
                                  .and_then(|words| words.checked_add(1))
                                  .and_then(|words| words.checked_add(envc))
                                  .and_then(|words| words.checked_add(1));
    let Some(auxv_addr) = words_before_auxv.and_then(|words| words.checked_mul(word))
                                           .and_then(|bytes| sp.checked_add(bytes))
    else {
        log::error!("[execve][qemu-host-cap] arch={} path={} aspace={:#x} sp={:#x} \
                     auxv_addr=overflow",
                    current_arch_name(),
                    path,
                    elf.user_aspace_ptr,
                    sp);
        return;
    };

    let ops = ActiveUserMemoryOps::new(elf.user_aspace_ptr);
    match read_auxv_hwcap(&ops, auxv_addr) {
        Ok(Some(hwcap)) => {
            log::error!("[execve][qemu-host-cap] arch={} path={} aspace={:#x} sp={:#x} \
                         auxv={:#x} at_hwcap={:#x} bit2={}",
                        current_arch_name(),
                        path,
                        elf.user_aspace_ptr,
                        sp,
                        auxv_addr,
                        hwcap,
                        hwcap & (1 << 2) != 0);
        }
        Ok(None) => {
            log::error!("[execve][qemu-host-cap] arch={} path={} aspace={:#x} sp={:#x} \
                         auxv={:#x} at_hwcap=missing",
                        current_arch_name(),
                        path,
                        elf.user_aspace_ptr,
                        sp,
                        auxv_addr);
        }
        Err(()) => {
            log::error!("[execve][qemu-host-cap] arch={} path={} aspace={:#x} sp={:#x} \
                         auxv={:#x} at_hwcap=unreadable",
                        current_arch_name(),
                        path,
                        elf.user_aspace_ptr,
                        sp,
                        auxv_addr);
        }
    }
}

fn read_auxv_hwcap<Ops : UserMemoryOps>(ops : &Ops,
                                        auxv_addr : usize)
                                        -> Result<Option<usize>, ()> {
    let word = core::mem::size_of::<usize>();
    let pair_size = word.checked_mul(2)
                        .ok_or(())?;
    for pair_index in 0..AUXV_DIAGNOSTIC_MAX_PAIRS {
        let pair_addr = pair_index.checked_mul(pair_size)
                                  .and_then(|offset| auxv_addr.checked_add(offset))
                                  .ok_or(())?;
        let mut pair = [0u8; core::mem::size_of::<usize>() * 2];
        let copied = ops.copy_from_user(&mut pair,
                                        mm::api::addr::VirtAddr(pair_addr))
                        .map_err(|_| ())?;
        if copied != pair.len() {
            return Err(());
        }
        let mut tag_bytes = [0u8; core::mem::size_of::<usize>()];
        let mut value_bytes = [0u8; core::mem::size_of::<usize>()];
        tag_bytes.copy_from_slice(&pair[..word]);
        value_bytes.copy_from_slice(&pair[word..]);
        let tag = usize::from_le_bytes(tag_bytes);
        let value = usize::from_le_bytes(value_bytes);
        if tag == AT_NULL {
            return Ok(None);
        }
        if tag == AT_HWCAP {
            return Ok(Some(value));
        }
    }
    Err(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_direct_and_dynamic_loader_qemu_invocations() {
        assert_eq!(qemu_system_invocation("/opt/qemu/bin/qemu-system-loongarch64",
                                          &[]),
                   Some("/opt/qemu/bin/qemu-system-loongarch64"));

        let loader_argv = ["/opt/qemu/lib/ld-linux-loongarch-lp64d.so.1",
                           "--library-path",
                           "/opt/qemu/lib",
                           "/opt/qemu/bin/qemu-system-loongarch64",
                           "-machine",
                           "virt"];
        assert_eq!(qemu_system_invocation(loader_argv[0], &loader_argv),
                   Some("/opt/qemu/bin/qemu-system-loongarch64"));
        assert_eq!(qemu_system_invocation("/bin/sh", &["sh", "test.sh"]),
                   None);
    }
}

const fn current_arch_name() -> &'static str {
    #[cfg(target_arch = "riscv64")]
    {
        "riscv64"
    }
    #[cfg(target_arch = "loongarch64")]
    {
        "loongarch64"
    }
    #[cfg(not(any(target_arch = "riscv64", target_arch = "loongarch64")))]
    {
        "unknown"
    }
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
