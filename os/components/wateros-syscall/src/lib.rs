#![no_std]
#![allow(static_mut_refs)]
//! 用户态系统调用分发：将 ABI
//! 规定的调用号与寄存器参数映射到内核任务、控制台与最小内存桩。
//!
//! **契约**：[`dispatch_syscall_from_trap`] 为 Rust 侧 trap
//! 组合入口；[`__wateros_syscall_dispatch_current`] 供 C ABI（如 `switch`
//! 桩）使用。返回值遵循 `UserRet`/`ErrNo` 编码。
//!
//! **依赖与 feature**：`abi` / `task` 由 crate feature（如
//! `impl-riscv64`、`impl-loongarch64`）选择具体平台表与调度实现；`console`
//! 提供内核侧原始字节输出。未实现的调用统一返回 `ENOSYS`。

extern crate alloc;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::syscall_number::{ActiveSyscallNumberTable, SyscallNumberTable};
use abi::user_ret::UserRet;
use alloc::vec::Vec;
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicUsize, Ordering};

// 与当前 `ActiveSyscallNumberTable` 一致的调用号常量，供 `match` 与 trap
// 侧传入的 `syscall_nr` 做整数比较（避免运行时查表）。
const SYSCALL_YIELD_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::YIELD.raw();
const SYSCALL_EXIT_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT.raw();
const SYSCALL_EXIT_GROUP_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::EXIT_GROUP.raw();
const SYSCALL_READ_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::READ.raw();
const SYSCALL_WRITE_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::WRITE.raw();
const SYSCALL_CLOSE_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::CLOSE.raw();
const SYSCALL_PIPE2_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::PIPE2.raw();
const SYSCALL_BRK_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::BRK.raw();
const SYSCALL_MMAP_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::MMAP.raw();
const SYSCALL_MUNMAP_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::MUNMAP.raw();
const SYSCALL_MPROTECT_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::MPROTECT.raw();
const SYSCALL_WAITPID_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::WAITPID.raw();
const SYSCALL_GET_TIME_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::GET_TIME.raw();
const SYSCALL_GETPID_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::GETPID.raw();
const SYSCALL_GETTID_NR : usize = <ActiveSyscallNumberTable as SyscallNumberTable>::GETTID.raw();
const SYSCALL_NANOSLEEP_NR : usize =
    <ActiveSyscallNumberTable as SyscallNumberTable>::NANOSLEEP.raw();

/// 用户态 `brk` 的单调递增假顶：在无 ELF
/// 用户页表（`user_aspace_ptr==0`）时兜底。
static USER_BRK_FAKE : AtomicUsize = AtomicUsize::new(0);
static mut FD_REGISTRY : MaybeUninit<UniprocessorSafeCell<FdRegistry>> = MaybeUninit::uninit();
static FD_REGISTRY_READY : AtomicUsize = AtomicUsize::new(0);

const STDIN_FD : usize = 0;
const STDOUT_FD : usize = 1;
const STDERR_FD : usize = 2;
const FIRST_DYNAMIC_FD : usize = 3;
const O_NONBLOCK : usize = 0o0004000;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserTimespec {
    sec : isize,
    nsec : isize,
}

#[derive(Clone)]
enum FdEntry {
    ConsoleIn,
    ConsoleOut,
    Pipe(ipc::pipe::PipeEndpoint),
}

struct FdRegistry {
    tables : Vec<Vec<Option<FdEntry>>>,
}

impl FdRegistry {
    fn new() -> Self { Self { tables : Vec::new() } }

    fn table_mut(&mut self, task_id : task::TaskId) -> &mut Vec<Option<FdEntry>> {
        if self.tables.len() <= task_id {
            self.tables
                .resize_with(task_id + 1, Vec::new);
        }
        let table = &mut self.tables[task_id];
        if table.len() < FIRST_DYNAMIC_FD {
            table.resize_with(FIRST_DYNAMIC_FD, || None);
            table[STDIN_FD] = Some(FdEntry::ConsoleIn);
            table[STDOUT_FD] = Some(FdEntry::ConsoleOut);
            table[STDERR_FD] = Some(FdEntry::ConsoleOut);
        }
        table
    }

    fn get(&mut self, task_id : task::TaskId, fd : usize) -> Option<FdEntry> {
        self.table_mut(task_id)
            .get(fd)
            .and_then(Clone::clone)
    }

    fn alloc(&mut self, task_id : task::TaskId, entry : FdEntry) -> usize {
        let table = self.table_mut(task_id);
        for fd in FIRST_DYNAMIC_FD..table.len() {
            if table[fd].is_none() {
                table[fd] = Some(entry);
                return fd;
            }
        }
        table.push(Some(entry));
        table.len() - 1
    }

    fn close(&mut self, task_id : task::TaskId, fd : usize) -> Option<FdEntry> {
        if fd < FIRST_DYNAMIC_FD {
            return None;
        }
        self.table_mut(task_id)
            .get_mut(fd)
            .and_then(Option::take)
    }
}

fn fd_registry_cell() -> &'static UniprocessorSafeCell<FdRegistry> {
    if FD_REGISTRY_READY.load(Ordering::Acquire) == 0 {
        unsafe {
            FD_REGISTRY.write(UniprocessorSafeCell::new(FdRegistry::new()));
        }
        FD_REGISTRY_READY.store(1, Ordering::Release);
    }
    unsafe { &*FD_REGISTRY.as_ptr() }
}

fn current_task_id_or_errno() -> Result<task::TaskId, ErrNo> {
    task::current_task_id().ok_or(ErrNo::ESRCH)
}

fn fd_entry(fd : usize) -> Result<FdEntry, ErrNo> {
    let task_id = current_task_id_or_errno()?;
    fd_registry_cell()
        .exclusive_access()
        .get(task_id, fd)
        .ok_or(ErrNo::EBADF)
}

fn pipe_err_to_errno(err : ipc::pipe::PipeError) -> ErrNo {
    match err {
        ipc::pipe::PipeError::WouldBlock => ErrNo::EAGAIN,
        ipc::pipe::PipeError::BrokenPipe => ErrNo::EPIPE,
        ipc::pipe::PipeError::InvalidCapacity => ErrNo::EINVAL,
    }
}

// `read`：支持 pipe read endpoint；stdin 暂未接真实输入。
#[inline]
fn dispatch_read(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1) as *mut u8;
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr.is_null() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > 4 * 1024 * 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let entry = match fd_entry(fd) {
        Ok(entry) => entry,
        Err(err) => return UserRet::from_error(err),
    };
    let buf = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
    match entry {
        FdEntry::Pipe(endpoint) => {
            if endpoint.kind() != ipc::pipe::PipeEndpointKind::Read {
                return UserRet::from_error(ErrNo::EBADF);
            }
            endpoint.read(buf)
                    .map(UserRet::from_success)
                    .unwrap_or_else(|err| UserRet::from_error(pipe_err_to_errno(err)))
        }
        _ => UserRet::from_error(ErrNo::EBADF),
    }
}

// `write`：fd 1/2 走控制台；pipe write endpoint 走 IPC pipe。
#[inline]
fn dispatch_write(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1) as *const u8;
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr.is_null() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > 4 * 1024 * 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let buf = unsafe { core::slice::from_raw_parts(ptr, len) };
    let entry = match fd_entry(fd) {
        Ok(entry) => entry,
        Err(err) => return UserRet::from_error(err),
    };
    match entry {
        FdEntry::ConsoleOut => {
            console::write_raw_bytes(buf);
            UserRet::from_success(len)
        }
        FdEntry::Pipe(endpoint) => {
            if endpoint.kind() != ipc::pipe::PipeEndpointKind::Write {
                return UserRet::from_error(ErrNo::EBADF);
            }
            endpoint.write(buf)
                    .map(UserRet::from_success)
                    .unwrap_or_else(|err| UserRet::from_error(pipe_err_to_errno(err)))
        }
        _ => UserRet::from_error(ErrNo::EBADF),
    }
}

#[inline]
fn dispatch_close(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let task_id = match current_task_id_or_errno() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(err),
    };
    let entry = fd_registry_cell()
        .exclusive_access()
        .close(task_id, fd);
    match entry {
        Some(FdEntry::Pipe(endpoint)) => {
            endpoint.close();
            UserRet::from_success(0)
        }
        Some(_) => UserRet::from_success(0),
        None => UserRet::from_error(ErrNo::EBADF),
    }
}

#[inline]
fn dispatch_pipe2(args : SyscallArgs) -> UserRet {
    let pipefd_ptr = args.arg(0) as *mut i32;
    let flags = args.arg(1);
    if pipefd_ptr.is_null() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & !O_NONBLOCK != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match current_task_id_or_errno() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(err),
    };
    let nonblocking = flags & O_NONBLOCK != 0;
    let (read_end, write_end) = ipc::pipe::PipeEndpoint::pair(nonblocking);
    let mut registry = fd_registry_cell()
        .exclusive_access();
    let read_fd = registry.alloc(task_id, FdEntry::Pipe(read_end));
    let write_fd = registry.alloc(task_id, FdEntry::Pipe(write_end));
    drop(registry);
    unsafe {
        pipefd_ptr.write(read_fd as i32);
        pipefd_ptr.add(1).write(write_fd as i32);
    }
    UserRet::from_success(0)
}

// `brk`：优先走 Sv39
// 用户页表（`LoadedElf::user_aspace_ptr`）；否则使用假堆顶桩。
#[cfg(feature = "impl-riscv64")]
#[inline]
fn mm_err_to_errno(e : mm::api::error::MmError) -> ErrNo {
    use mm::api::error::MmError;
    match e {
        MmError::OutOfMemory | MmError::FrameAlloc(_) => ErrNo::ENOMEM,
        MmError::InvalidAddress | MmError::AlreadyMapped | MmError::NotMapped => ErrNo::EINVAL,
        MmError::AccessViolation => ErrNo::EFAULT,
        MmError::Unsupported => ErrNo::ENOSYS,
    }
}

#[cfg(feature = "impl-riscv64")]
#[inline]
fn linux_mmap_prot_to_perm(prot : i32) -> mm::api::perm::PagePerm {
    use mm::api::perm::PagePerm;
    let mut p = PagePerm::empty();
    if prot & 1 != 0 {
        p |= PagePerm::R;
    }
    if prot & 2 != 0 {
        p |= PagePerm::W;
    }
    if prot & 4 != 0 {
        p |= PagePerm::X;
    }
    p
}

#[cfg(feature = "impl-riscv64")]
#[inline]
fn current_user_aspace_ptr() -> Option<usize> {
    let snap = task::current_task_snapshot()?;
    let ur = snap.user_resources?;
    let p = ur.user_aspace_ptr;
    if p == 0 {
        None
    } else {
        Some(p)
    }
}

#[cfg(feature = "impl-riscv64")]
#[inline]
fn dispatch_brk_sv39(addr : usize) -> Option<UserRet> {
    let ptr = current_user_aspace_ptr()?;
    Some(match mm::user_sv39_syscall::brk(ptr, addr) {
             Ok(v) => UserRet::from_success(v),
             Err(e) => UserRet::from_error(mm_err_to_errno(e)),
         })
}

#[inline]
fn dispatch_brk_fake(addr : usize) -> UserRet {
    // 须高于静态链接用户镜像末端（含大 `.bss` 堆）；仅作 `brk(0)` 查询桩。
    const INITIAL : usize = 0x0120_0000;
    if addr == 0 {
        let v = USER_BRK_FAKE.load(Ordering::Relaxed);
        if v == 0 {
            USER_BRK_FAKE.store(INITIAL, Ordering::Relaxed);
            return UserRet::from_success(INITIAL);
        }
        return UserRet::from_success(v);
    }
    let cur = USER_BRK_FAKE.load(Ordering::Relaxed)
                           .max(INITIAL);
    if addr < cur {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    USER_BRK_FAKE.store(addr, Ordering::Relaxed);
    UserRet::from_success(addr)
}

#[inline]
fn dispatch_brk(addr : usize) -> UserRet {
    #[cfg(feature = "impl-riscv64")]
    if let Some(r) = dispatch_brk_sv39(addr) {
        return r;
    }
    dispatch_brk_fake(addr)
}

#[cfg(feature = "impl-riscv64")]
#[inline]
fn dispatch_mmap(args : SyscallArgs) -> UserRet {
    let Some(ptr) = current_user_aspace_ptr() else {
        return UserRet::from_error(ErrNo::ENOSYS);
    };
    let addr = args.arg(0);
    let len = args.arg(1);
    let prot = args.arg(2) as i32;
    let flags = args.arg(3) as u32;
    let fd = args.arg(4);
    let offset = args.arg(5);
    let perm = linux_mmap_prot_to_perm(prot);
    match mm::user_sv39_syscall::mmap(ptr, addr, len, perm, flags, fd, offset) {
        Ok(base) => UserRet::from_success(base),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

#[cfg(not(feature = "impl-riscv64"))]
#[inline]
fn dispatch_mmap(_args : SyscallArgs) -> UserRet { UserRet::from_error(ErrNo::ENOSYS) }

#[cfg(feature = "impl-riscv64")]
#[inline]
fn dispatch_munmap(args : SyscallArgs) -> UserRet {
    let Some(ptr) = current_user_aspace_ptr() else {
        return UserRet::from_error(ErrNo::ENOSYS);
    };
    let addr = args.arg(0);
    let len = args.arg(1);
    match mm::user_sv39_syscall::munmap(ptr, addr, len) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

#[cfg(not(feature = "impl-riscv64"))]
#[inline]
fn dispatch_munmap(_args : SyscallArgs) -> UserRet { UserRet::from_error(ErrNo::ENOSYS) }

#[cfg(feature = "impl-riscv64")]
#[inline]
fn dispatch_mprotect(args : SyscallArgs) -> UserRet {
    let Some(ptr) = current_user_aspace_ptr() else {
        return UserRet::from_error(ErrNo::ENOSYS);
    };
    let addr = args.arg(0);
    let len = args.arg(1);
    let prot = args.arg(2) as i32;
    let perm = linux_mmap_prot_to_perm(prot);
    match mm::user_sv39_syscall::mprotect(ptr, addr, len, perm) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(mm_err_to_errno(e)),
    }
}

#[cfg(not(feature = "impl-riscv64"))]
#[inline]
fn dispatch_mprotect(_args : SyscallArgs) -> UserRet { UserRet::from_error(ErrNo::ENOSYS) }

#[inline]
fn dispatch_current_task_id() -> UserRet {
    task::current_task_id().map(UserRet::from_success)
                           .unwrap_or_else(|| UserRet::from_error(ErrNo::ESRCH))
}

#[inline]
fn write_exit_code(exit_code_ptr : usize, exit_code : isize) {
    if exit_code_ptr != 0 {
        unsafe {
            (exit_code_ptr as *mut i32).write(exit_code as i32);
        }
    }
}

#[inline]
fn finish_wait_result(exited : task::ExitedTask, exit_code_ptr : usize) -> UserRet {
    write_exit_code(exit_code_ptr, exited.exit_code);
    UserRet::from_success(exited.id)
}

// `waitpid`/`wait4` 早期语义：维护最小父子关系并阻塞等待子任务退出；暂不解析 options。
#[inline]
fn dispatch_waitpid(args : SyscallArgs) -> UserRet {
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

// `nanosleep` 临时映射到一个调度 tick；真实时间换算待平台频率语义接入后再替换。
#[inline]
fn dispatch_nanosleep(args : SyscallArgs) -> UserRet {
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

/// Trap / 异常返回路径上应调用的系统调用分发（具名 Rust API；组合层
/// `trap_handler` 直接调用本函数）。
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    match syscall_nr {
        SYSCALL_YIELD_NR => {
            // 协作式让出当前任务；无额外参数。
            task::yield_now();
            UserRet::from_success(0).0
        }
        SYSCALL_EXIT_NR | SYSCALL_EXIT_GROUP_NR => {
            // 单进程模型下 `EXIT` 与 `EXIT_GROUP` 均视为终止当前任务。
            let exit_code = syscall_args.arg(0) as isize;
            task::exit_current(exit_code)
        }
        SYSCALL_READ_NR => dispatch_read(syscall_args).0,
        SYSCALL_WRITE_NR => dispatch_write(syscall_args).0,
        SYSCALL_CLOSE_NR => dispatch_close(syscall_args).0,
        SYSCALL_PIPE2_NR => dispatch_pipe2(syscall_args).0,
        SYSCALL_BRK_NR => dispatch_brk(syscall_args.arg(0)).0,
        SYSCALL_MMAP_NR => dispatch_mmap(syscall_args).0,
        SYSCALL_MUNMAP_NR => dispatch_munmap(syscall_args).0,
        SYSCALL_MPROTECT_NR => dispatch_mprotect(syscall_args).0,
        SYSCALL_GET_TIME_NR => UserRet::from_success(task::current_tick() as usize).0,
        SYSCALL_GETPID_NR | SYSCALL_GETTID_NR => dispatch_current_task_id().0,
        SYSCALL_WAITPID_NR => dispatch_waitpid(syscall_args).0,
        SYSCALL_NANOSLEEP_NR => dispatch_nanosleep(syscall_args).0,
        // 未在表中实现的调用：保持与 Linux 风格 `ENOSYS` 一致，便于用户态探测能力。
        _ => UserRet::from_error(ErrNo::ENOSYS).0,
    }
}

/// 当前任务上的系统调用分发入口：按 `ActiveSyscallNumberTable` 解析
/// `syscall_nr`，参数来自通用寄存器约定。
///
/// 已识别调用：`YIELD`、`EXIT`/`EXIT_GROUP`、`WRITE`、`BRK`（Sv39
/// 真路径或假顶桩）、 `MMAP`/`MUNMAP`/`MPROTECT`（RISC-V + `user_aspace_ptr`
/// 时）、 `GET_TIME`、`GETPID`/`GETTID`、`WAITPID` 与 `NANOSLEEP`；其余返回
/// `ENOSYS`。
///
/// **ABI**：`extern "C"` 且 `#[unsafe(no_mangle)]`，符号名固定，供汇编或 C
/// 侧按平台调用约定直接跳转；六个 `usize` 与 `SyscallArgs::from_regs`
/// 所用寄存器槽顺序一致。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_syscall_dispatch_current(syscall_nr : usize,
                                                     arg0 : usize,
                                                     arg1 : usize,
                                                     arg2 : usize,
                                                     arg3 : usize,
                                                     arg4 : usize,
                                                     arg5 : usize)
                                                     -> isize {
    let syscall_args = SyscallArgs::from_regs([arg0, arg1, arg2, arg3, arg4, arg5]);
    dispatch_syscall_from_trap(syscall_nr, syscall_args)
}
